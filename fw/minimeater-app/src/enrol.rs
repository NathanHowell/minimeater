//! Enrolment of the device key (`minimeat-seq.4.34`, ADR 0005), as the serial REPL asks for it: look at the secure
//! element, provision it (one way) and print the certificate signing request. The REPL sends a [`Job`] on the bus; the task
//! that owns the chip ([`serve`]) answers with lines of text and an [`Out::End`]. The flow itself is
//! [`crate::drivers::identity`]; nothing here reaches the network, and no board without the chip (the twin) runs a task.

use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;
use heapless::String;

use crate::drivers::atecc608::Atecc608;
use crate::drivers::csr::{self, ToBeSigned};
use crate::drivers::identity::{self, Fault, KEY_SLOT};
use crate::tasks::Bus;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    /// Report the chip and what provisioning would do; change nothing.
    Look,
    /// Do it: write the configuration, lock it, make the key, lock the data zone. One way.
    Provision,
    /// The certificate signing request for the key, signed by the chip.
    Csr,
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
pub async fn serve<I: I2c, T: DelayNs>(chip: &mut Atecc608<I, T>, bus: &Bus, cn: &str) -> ! {
    loop {
        let job = bus.enrol_job.receive().await;
        match job {
            Job::Look | Job::Provision => {
                let r = identity::provision(chip, job == Job::Provision, &mut async |l: &str| put(bus, l).await).await;
                if let Err(f) = r {
                    put(bus, fault(f)).await;
                }
            }
            Job::Csr => csr_job(chip, bus, cn).await,
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
    fn type_line(bus: &Bus, chip: Option<&mut Atecc608<Chip, NoDelay>>, line: &str) -> Str {
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
        let mut chip = Atecc608::new(Chip::new(blank()), ADDRESS, NoDelay);
        // looking changes nothing
        let t = type_line(&bus, Some(&mut chip), "identity");
        assert!(
            t.contains("configuration open, data open") && t.contains("not done"),
            "{t}"
        );
        assert!(chip.i2c().config == blank());
        // the word that locks is needed
        let t = type_line(&bus, Some(&mut chip), "identity provision");
        assert!(
            t.contains("locks the secure element for good") && chip.i2c().config == blank(),
            "{t}"
        );
        // no key, no request
        let t = type_line(&bus, Some(&mut chip), "identity csr");
        assert!(t.contains("no key yet"), "{t}");
        // locked in
        let t = type_line(&bus, Some(&mut chip), "identity provision lock");
        assert!(t.contains("data zone locked") && t.contains("public key"), "{t}");
        // and now the request
        let t = type_line(&bus, Some(&mut chip), "identity csr");
        assert!(
            t.contains("-----BEGIN CERTIFICATE REQUEST-----") && t.contains("-----END CERTIFICATE REQUEST-----"),
            "{t}"
        );
        // a second look reads the key again
        let t = type_line(&bus, Some(&mut chip), "identity");
        assert!(
            t.contains("configuration locked, data locked") && t.contains("public key"),
            "{t}"
        );
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
