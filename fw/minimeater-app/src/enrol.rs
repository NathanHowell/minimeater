//! Enrolment of the device key (`minimeat-seq.4.34`, ADR 0005), as the serial REPL asks for it: look at the secure
//! element, provision it (one way) and print the certificate signing request. The REPL sends a [`Job`] on the bus; the task
//! that owns the chip ([`serve`]) answers with lines of text and an [`Out::End`]. The flow itself is
//! [`crate::drivers::identity`]; nothing here reaches the network, and no board without the chip (the twin) runs a task.

use core::fmt::Write as _;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;
use heapless::{String, Vec};

use crate::drivers::atecc608::Atecc608;
use crate::drivers::cert::{self, Der};
use crate::drivers::csr::{self, ToBeSigned};
use crate::drivers::identity::{self, Fault, KEY_SLOT};
use crate::tasks::Bus;

/// Bytes of certificate one `identity cert add` carries (a 64-character word of base64).
pub const CHUNK: usize = 48;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    /// Report the chip and what provisioning would do; change nothing.
    Look,
    /// Do it: write the configuration, lock it, make the key, lock the data zone. One way.
    Provision,
    /// The certificate signing request for the key, signed by the chip.
    Csr,
    /// Erase the certificate and make a new key ([`identity::rekey`]).
    Rekey,
    /// Print the certificate in the chip's certificate slot as PEM.
    CertShow,
    /// Forget any certificate bytes received so far.
    CertStart,
    /// Append bytes of the DER certificate.
    CertAdd(Vec<u8, CHUNK>),
    /// Check what was received against the chip's key and write it to the slot.
    CertSave,
}

/// The secure element, shared by this task and the TLS signer ([`crate::tls::ChipSigner`]): whoever holds the lock has the chip
/// for its whole job, so two sessions never interleave on the bus.
pub type Shared<I, T> = Mutex<CriticalSectionRawMutex, Atecc608<I, T>>;

/// Puts the chip behind the lock.
pub const fn share<I, T>(chip: Atecc608<I, T>) -> Shared<I, T> {
    Mutex::new(chip)
}

pub type Line = String<96>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Out {
    Line(Line),
    /// The job is over.
    End,
}

async fn put(bus: &Bus, text: &str) {
    let mut l = Line::new();
    for c in text.chars() {
        if l.push(c).is_err() {
            break;
        }
    }
    bus.enrol_out.send(Out::Line(l)).await;
}

fn fault(f: Fault) -> &'static str {
    match f {
        Fault::Chip(_) => "error: the chip did not answer as expected",
        Fault::WrongChip => "error: this is not an ATECC608",
        Fault::Different => "error: the chip is locked with another configuration",
        Fault::Verify(_) => "error: the configuration read back wrong; not locked",
        Fault::NotLocked => "error: a lock did not take",
        Fault::NotProvisioned => "error: this chip is not provisioned (identity provision lock first)",
    }
}

fn cert_fault(f: cert::Fault) -> &'static str {
    use cert::Fault::*;
    match f {
        Chip(_) => "error: the chip did not answer as expected",
        TooLong => "error: too long: the slot holds a certificate of at most 414 bytes",
        NotCertificate => "error: that is not a DER certificate (send the DER, as base64 without the PEM lines)",
        WrongKey => "error: that certificate is for a different key; the slot is unchanged",
        Differs => "error: the slot did not read back as written",
        NoKey => "error: no key in the chip yet (identity provision lock)",
    }
}

async fn cert_show<I: I2c, T: DelayNs>(chip: &mut Atecc608<I, T>, bus: &Bus) {
    match cert::load(chip).await {
        Err(_) => put(bus, "error: the chip did not answer as expected").await,
        Ok(None) => put(bus, "no certificate stored (identity cert add, then save)").await,
        Ok(Some(der)) => match csr::pem::<12>("CERTIFICATE", &der) {
            Some(lines) => {
                for l in lines {
                    put(bus, &l).await;
                }
            }
            None => put(bus, "error: the certificate does not fit a printout").await,
        },
    }
}

async fn csr_job<I: I2c, T: DelayNs>(chip: &mut Atecc608<I, T>, bus: &Bus, cn: &str) {
    let Ok(key) = chip.public_key(KEY_SLOT).await else {
        put(bus, "no key yet (identity provision)").await;
        return;
    };
    let Some(tbs) = ToBeSigned::new(cn, &key) else {
        put(bus, "error: the device name does not fit a request").await;
        return;
    };
    let Ok(sig) = chip.sign(KEY_SLOT, &tbs.digest()).await else {
        put(bus, "error: the chip would not sign").await;
        return;
    };
    let Some(der) = tbs.finish(&sig) else {
        put(bus, "error: the request does not fit").await;
        return;
    };
    match csr::pem::<12>("CERTIFICATE REQUEST", &der) {
        Some(lines) => {
            for l in lines {
                put(bus, &l).await;
            }
        }
        None => put(bus, "error: the request does not fit").await,
    }
}

/// Runs the jobs the REPL sends, for ever. `cn` is the device's name for the request's subject.
pub async fn serve<I: I2c, T: DelayNs>(shared: &Shared<I, T>, bus: &Bus, cn: &str) -> ! {
    // the certificate as it arrives, until it is saved
    let mut staged = Der::new();
    loop {
        let job = bus.enrol_job.receive().await;
        // (a job is a few chip sessions: the TLS signer waits out of the way, or fails its handshake and retries)
        let mut guard = shared.lock().await;
        let chip = &mut *guard;
        match job {
            Job::Look | Job::Provision => {
                let r = identity::provision(chip, job == Job::Provision, &mut async |l: &str| put(bus, l).await).await;
                if let Err(f) = r {
                    put(bus, fault(f)).await;
                }
            }
            Job::Csr => csr_job(chip, bus, cn).await,
            Job::Rekey => {
                if let Err(f) = identity::rekey(chip, &mut async |l: &str| put(bus, l).await).await {
                    put(bus, fault(f)).await;
                }
            }
            Job::CertShow => cert_show(chip, bus).await,
            Job::CertStart => {
                staged.clear();
                put(bus, "ready for a certificate").await;
            }
            Job::CertAdd(bytes) => {
                if staged.extend_from_slice(&bytes).is_ok() {
                    let mut l = Line::new();
                    let _ = write!(l, "{} bytes so far", staged.len());
                    bus.enrol_out.send(Out::Line(l)).await;
                } else {
                    staged.clear();
                    put(
                        bus,
                        "error: too long: the slot holds a certificate of at most 414 bytes; start again",
                    )
                    .await;
                }
            }
            Job::CertSave => {
                match cert::store(chip, &staged).await {
                    Ok(()) => {
                        let mut l = Line::new();
                        let _ = write!(l, "certificate saved: {} bytes, read back equal", staged.len());
                        bus.enrol_out.send(Out::Line(l)).await;
                        staged.clear();
                    }
                    // what was received stays (`identity cert start` clears it)
                    Err(f) => put(bus, cert_fault(f)).await,
                }
            }
        }
        bus.enrol_out.send(Out::End).await;
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::string::String as Str;
    use std::vec::Vec;

    use embassy_futures::block_on;
    use embassy_futures::select::select;
    use embedded_io_async::{ErrorType, Write};

    use super::*;
    use crate::drivers::atecc608::ADDRESS;
    use crate::drivers::cert::tests::fake;
    use crate::drivers::identity::tests::{Chip, blank};
    use crate::drivers::mock::NoDelay;
    use crate::net::Identity;
    use crate::repl;

    struct Out(Vec<u8>);
    impl ErrorType for Out {
        type Error = core::convert::Infallible;
    }
    impl Write for Out {
        async fn write(&mut self, b: &[u8]) -> Result<usize, Self::Error> {
            self.0.extend_from_slice(b);
            Ok(b.len())
        }
    }

    const WHO: Identity<'static> = Identity {
        prefix: "minimeater",
        id: "mm-a1b2c3",
        sw: "t",
        hw: "A",
    };

    /// Types `line` at the REPL with a chip task behind the bus (or none).
    fn type_line(bus: &Bus, chip: Option<&Shared<Chip, NoDelay>>, line: &str) -> Str {
        let mut out = Out(Vec::new());
        let mut replies = bus.replies.subscriber().unwrap();
        let ask = repl::execute(&mut out, line, &WHO, bus, &mut replies);
        match chip {
            Some(chip) => {
                // (serve never returns; the REPL's side ends the race)
                let _ = block_on(select(serve(chip, bus, "mm-a1b2c3"), ask));
            }
            None => block_on(ask),
        }
        Str::from_utf8(out.0).unwrap()
    }

    #[test]
    fn the_repl_looks_provisions_and_prints_a_request_through_the_chip_task() {
        let bus = Bus::new();
        let chip: Shared<Chip, NoDelay> = Mutex::new(Atecc608::new(Chip::new(blank()), ADDRESS, NoDelay));
        // looking changes nothing
        let t = type_line(&bus, Some(&chip), "identity");
        assert!(
            t.contains("configuration open, data open") && t.contains("not done"),
            "{t}"
        );
        assert!(block_on(chip.lock()).i2c().config == blank());
        // the word that locks is needed
        let t = type_line(&bus, Some(&chip), "identity provision");
        assert!(
            t.contains("locks the secure element for good") && block_on(chip.lock()).i2c().config == blank(),
            "{t}"
        );
        // no key, no request
        let t = type_line(&bus, Some(&chip), "identity csr");
        assert!(t.contains("no key yet"), "{t}");
        // locked in
        let t = type_line(&bus, Some(&chip), "identity provision lock");
        assert!(t.contains("data zone locked") && t.contains("public key"), "{t}");
        // and now the request
        let t = type_line(&bus, Some(&chip), "identity csr");
        assert!(
            t.contains("-----BEGIN CERTIFICATE REQUEST-----") && t.contains("-----END CERTIFICATE REQUEST-----"),
            "{t}"
        );
        // a second look reads the key again
        let t = type_line(&bus, Some(&chip), "identity");
        assert!(
            t.contains("configuration locked, data locked") && t.contains("public key"),
            "{t}"
        );
    }

    /// The words `identity cert add` takes for `der`.
    fn words(der: &[u8]) -> std::vec::Vec<Str> {
        use base64ct::{Base64, Encoding as _};
        der.chunks(CHUNK)
            .map(|c| {
                let mut buf = [0u8; 64];
                Str::from(Base64::encode(c, &mut buf).unwrap())
            })
            .collect()
    }

    /// Types `lines` one after another at one REPL with one chip task behind it (what the task has received, as a
    /// certificate arrives, lives as long as the task does), and returns what each printed.
    fn type_lines(bus: &Bus, chip: &Shared<Chip, NoDelay>, lines: &[Str]) -> std::vec::Vec<Str> {
        let mut replies = bus.replies.subscriber().unwrap();
        let typed = async {
            let mut all = std::vec::Vec::new();
            for line in lines {
                let mut out = Out(Vec::new());
                repl::execute(&mut out, line, &WHO, bus, &mut replies).await;
                all.push(Str::from_utf8(out.0).unwrap());
            }
            all
        };
        match block_on(select(serve(chip, bus, "mm-a1b2c3"), typed)) {
            embassy_futures::select::Either::Second(all) => all,
            embassy_futures::select::Either::First(never) => never,
        }
    }

    /// The lines that send `der` and save it.
    fn send(der: &[u8]) -> std::vec::Vec<Str> {
        let mut lines = std::vec!["identity cert start".into()];
        lines.extend(words(der).into_iter().map(|w| std::format!("identity cert add {w}")));
        lines.push("identity cert save".into());
        lines
    }

    #[test]
    fn a_certificate_is_loaded_word_by_word_checked_saved_and_printed_back() {
        let bus = Bus::new();
        let chip: Shared<Chip, NoDelay> = Mutex::new(Atecc608::new(Chip::new(blank()), ADDRESS, NoDelay));
        // no key, nothing to check a certificate against
        let t = type_line(&bus, Some(&chip), "identity cert save");
        assert!(t.contains("no key in the chip"), "{t}");
        type_line(&bus, Some(&chip), "identity provision lock");
        let t = type_line(&bus, Some(&chip), "identity cert");
        assert!(t.contains("no certificate stored"), "{t}");
        let key = block_on(async { chip.lock().await.public_key(KEY_SLOT).await }).unwrap();

        // a certificate for another board is refused, and the slot is untouched
        let t = type_lines(&bus, &chip, &send(&fake(&[9u8; 64], 150)));
        assert!(t.last().unwrap().contains("different key"), "{t:?}");
        assert_eq!(block_on(async { cert::load(&mut *chip.lock().await).await }), Ok(None));

        // starting again drops what was received; this one is for the chip
        let der = fake(&key, 250);
        let mut lines = send(&fake(&[9u8; 64], 150));
        lines.pop(); // (not saved)
        lines.extend(send(&der));
        let t = type_lines(&bus, &chip, &lines);
        let n = t.len();
        assert!(t[n - 2].contains(&std::format!("{} bytes so far", der.len())), "{t:?}");
        assert!(
            t[n - 1].contains("certificate saved") && t[n - 1].contains("read back equal"),
            "{t:?}"
        );
        assert_eq!(
            block_on(async { cert::load(&mut *chip.lock().await).await })
                .unwrap()
                .as_deref(),
            Some(&der[..])
        );

        // and printed from the chip
        let t = type_line(&bus, Some(&chip), "identity cert");
        assert!(
            t.contains("-----BEGIN CERTIFICATE-----") && t.contains("-----END CERTIFICATE-----"),
            "{t}"
        );
    }

    #[test]
    fn certificate_words_that_are_wrong_are_refused_where_they_are_typed() {
        let bus = Bus::new();
        let chip: Shared<Chip, NoDelay> = Mutex::new(Atecc608::new(Chip::new(blank()), ADDRESS, NoDelay));
        let t = type_line(&bus, Some(&chip), "identity cert add !!!!");
        assert!(t.contains("not base64"), "{t}");
        // more than the slot holds: said, and what was received is dropped
        let mut lines = std::vec!["identity cert start".into()];
        lines.extend(
            words(&[0x30u8; 432])
                .into_iter()
                .map(|w| std::format!("identity cert add {w}")),
        );
        let t = type_lines(&bus, &chip, &lines);
        assert!(t.last().unwrap().contains("too long"), "{t:?}");
    }

    /// Types `lines` at the REPL with a stand-in for the control task that answers every request, and returns what each
    /// printed and the requests that were made.
    fn type_with_control(bus: &Bus, lines: &[&str]) -> (std::vec::Vec<Str>, std::vec::Vec<crate::proto::Request>) {
        use crate::tasks::Reply;
        let mut replies = bus.replies.subscriber().unwrap();
        let typed = async {
            let mut all = std::vec::Vec::new();
            for line in lines {
                let mut out = Out(Vec::new());
                repl::execute(&mut out, line, &WHO, bus, &mut replies).await;
                all.push(Str::from_utf8(out.0).unwrap());
            }
            all
        };
        let seen = core::cell::RefCell::new(std::vec::Vec::new());
        let control = async {
            let publisher = bus.replies.immediate_publisher();
            loop {
                let inbound = bus.commands.receive().await;
                seen.borrow_mut().push(inbound.req);
                publisher.publish_immediate(Reply {
                    id: inbound.id,
                    result: Ok(()),
                });
            }
        };
        match block_on(select(control, typed)) {
            embassy_futures::select::Either::Second(all) => (all, seen.into_inner()),
            embassy_futures::select::Either::First(never) => never,
        }
    }

    #[test]
    fn the_ca_is_collected_in_words_checked_and_made_a_setting() {
        use crate::proto::Request;
        let bus = Bus::new();
        let der = fake(&[3u8; 64], 200);
        let mut lines: std::vec::Vec<Str> = std::vec!["mqtt ca start".into()];
        lines.extend(words(&der).into_iter().map(|w| std::format!("mqtt ca add {w}")));
        lines.push("mqtt ca save".into());
        lines.push("mqtt ca clear".into());
        let refs: std::vec::Vec<&str> = lines.iter().map(Str::as_str).collect();
        let (out, requests) = type_with_control(&bus, &refs);
        let n = out.len();
        assert!(out[0].contains("ready"), "{out:?}");
        assert!(
            out[n - 3].contains(&std::format!("{} bytes so far", der.len())),
            "{out:?}"
        );
        assert!(out[n - 2].contains("MQTT now uses TLS"), "{out:?}");
        assert!(out[n - 1].contains("plaintext"), "{out:?}");
        assert_eq!(requests.len(), 2, "{requests:?}");
        assert_eq!(requests, [Request::MqttCaSave, Request::MqttCaClear]);
        // the control task takes the staged bytes while it handles the request; the REPL clears them after
        assert_eq!(bus.ca_stage.lock(|c| c.borrow().clone()), crate::config::Ca::new());
        // what was saved is not kept for the next one
        let (out, requests) = type_with_control(&bus, &["mqtt ca save"]);
        assert!(
            out[0].contains("not a DER certificate") && requests.is_empty(),
            "{out:?}"
        );
    }

    #[test]
    fn a_bad_ca_never_becomes_a_request() {
        let bus = Bus::new();
        // a PEM file's text, as base64 words: not a DER sequence
        let pem = b"-----BEGIN CERTIFICATE-----\nMIIB";
        let mut lines: std::vec::Vec<Str> = std::vec!["mqtt ca start".into()];
        lines.extend(words(pem).into_iter().map(|w| std::format!("mqtt ca add {w}")));
        lines.push("mqtt ca save".into());
        lines.push("mqtt ca add !!!!".into());
        let refs: std::vec::Vec<&str> = lines.iter().map(Str::as_str).collect();
        let (out, requests) = type_with_control(&bus, &refs);
        let n = out.len();
        assert!(out[n - 2].contains("not a DER certificate"), "{out:?}");
        assert!(out[n - 1].contains("not base64"), "{out:?}");
        assert!(requests.is_empty(), "{requests:?}");
        // too long: said, and dropped
        let long = [0x30u8; 500];
        let mut lines: std::vec::Vec<Str> = std::vec!["mqtt ca start".into()];
        lines.extend(words(&long).into_iter().map(|w| std::format!("mqtt ca add {w}")));
        let refs: std::vec::Vec<&str> = lines.iter().map(Str::as_str).collect();
        let (out, _) = type_with_control(&bus, &refs);
        assert!(out.last().unwrap().contains("too long"), "{out:?}");
    }

    #[test]
    fn rekey_needs_its_word_and_then_the_certificate_is_gone_and_the_key_is_new() {
        let bus = Bus::new();
        let chip: Shared<Chip, NoDelay> = Mutex::new(Atecc608::new(Chip::new(blank()), ADDRESS, NoDelay));
        let t = type_line(&bus, Some(&chip), "identity rekey replace");
        assert!(t.contains("not provisioned"), "{t}");
        type_line(&bus, Some(&chip), "identity provision lock");
        let old = block_on(async { chip.lock().await.public_key(KEY_SLOT).await }).unwrap();
        let der = fake(&old, 100);
        let lines = send(&der);
        let t = type_lines(&bus, &chip, &lines);
        assert!(t.last().unwrap().contains("certificate saved"), "{t:?}");
        // without the word nothing happens
        let t = type_line(&bus, Some(&chip), "identity rekey");
        assert!(t.contains("identity rekey replace"), "{t}");
        assert_eq!(
            block_on(async { chip.lock().await.public_key(KEY_SLOT).await }).unwrap(),
            old
        );
        // with it
        let t = type_line(&bus, Some(&chip), "identity rekey replace");
        assert!(t.contains("certificate erased") && t.contains("new public key"), "{t}");
        let new = block_on(async { chip.lock().await.public_key(KEY_SLOT).await }).unwrap();
        assert_ne!(new, old);
        let t = type_line(&bus, Some(&chip), "identity cert");
        assert!(t.contains("no certificate stored"), "{t}");
        let t = type_line(&bus, Some(&chip), "identity csr");
        assert!(t.contains("BEGIN CERTIFICATE REQUEST"), "{t}");
    }

    #[test]
    fn with_no_chip_task_the_repl_gives_up_and_leaves_nothing_queued() {
        let bus = Bus::new();
        // (the timeout is real time on the host's embassy clock: use the mock driver's)
        let t = type_line(&bus, None, "identity");
        assert!(t.contains("no answer from the secure element"), "{t}");
        assert!(bus.enrol_job.try_receive().is_err(), "the job was taken back");
    }
}
