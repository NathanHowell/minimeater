//! TLS for the network clients (ADR 0003 and 0005, `minimeat-seq.5.5`): the pieces that put `embedded-tls` (a TLS 1.3 client,
//! `no_std`, no allocator) between a TCP connection and the MQTT or OTLP session, with the device's private key in the
//! secure element.
//!
//! - [`DeviceKey`] is the signer `embedded-tls` asks for when the server requests a client certificate: it hashes what
//!   TLS wants signed and has a [`DigestSigner`] (the ATECC608B, `drivers::atecc608`) sign the digest, then gives back the
//!   DER signature. The key never enters the firmware.
//! - [`Provider`] is the `embedded-tls` crypto provider: the random generator, the pinned CA that must have signed the
//!   server's certificate (so a server that is not ours is refused, whatever name it carries), the device key and the
//!   device certificate from slot 8 ([`crate::drivers::cert`]).
//! - [`Duplex`] makes the two halves of a connection ([`crate::net::Connector`], `embedded-io-async` 0.6) the one
//!   cloneable socket `embedded-tls` (`embedded-io-async` 0.7) wants, so the connection can be split into a reader and a
//!   writer for the session's `select`.
//!
//! Only the TLS 1.3 handshake with a certificate-signing ECDSA P-256 key is supported (what Mosquitto and the OTLP
//! collectors speak by default); nothing here reaches the network.

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_io_07::{ErrorKind as Kind07, ErrorType as ErrorType07};
use embedded_io_async::{Error as _, ErrorKind as Kind06, Read as Read06, Write as Write06};
use embedded_io_async_07::{Read as Read07, Write as Write07};
use embedded_tls::pki::CertVerifier;
use embedded_tls::{
    Aes128GcmSha256, Certificate, CryptoProvider, CryptoRngCore, SignatureScheme, TlsClock, TlsError, TlsVerifier,
};
use sha2::{Digest, Sha256};
use signature::SignerMut;

use crate::drivers::csr::{Der, ecdsa_sig};

/// Largest server certificate the verifier keeps while the handshake runs (it checks the signature at the end).
pub const SERVER_CERT: usize = 2048;
/// The TLS record buffers a connection needs: the largest record TLS allows (16 KiB and overhead).
pub const RECORD: usize = 16_640;

/// Something that signs a SHA-256 digest with the device key: the ATECC608B on the board, a software key in tests.
/// `embedded-tls` asks for signatures from non-async code, so this one is too; see `minimeat-b5w` for how the board makes
/// the chip's async bus fit.
pub trait DigestSigner {
    /// The ECDSA P-256 signature, r then s, or `Err` if the chip would not.
    fn sign(&mut self, digest: &[u8; 32]) -> Result<[u8; 64], SignFailed>;
}

#[derive(Debug, PartialEq, Eq)]
pub struct SignFailed;

/// The signer `embedded-tls` uses for the client certificate's `CertificateVerify`.
pub struct DeviceKey<'a, S>(&'a mut S);

impl<S: DigestSigner> SignerMut<Der> for DeviceKey<'_, S> {
    fn try_sign(&mut self, msg: &[u8]) -> Result<Der, signature::Error> {
        let digest: [u8; 32] = Sha256::digest(msg).into();
        let rs = self.0.sign(&digest).map_err(|_| signature::Error::new())?;
        ecdsa_sig(&rs).ok_or_else(signature::Error::new)
    }
}

/// What a TLS connection needs from the device: see the module comment. `ca` and `cert` are DER.
pub struct Provider<'a, R, S, C: TlsClock> {
    rng: R,
    verifier: CertVerifier<'a, Aes128GcmSha256, C, SERVER_CERT>,
    key: S,
    cert: &'a [u8],
}

impl<'a, R: CryptoRngCore, S: DigestSigner, C: TlsClock> Provider<'a, R, S, C> {
    pub fn new(rng: R, ca: &'a [u8], key: S, cert: &'a [u8]) -> Self {
        Self {
            rng,
            verifier: CertVerifier::new(Certificate::X509(ca)),
            key,
            cert,
        }
    }

    /// The signer back, e.g. to be used again for the next connection.
    pub fn into_key(self) -> S {
        self.key
    }
}

impl<R: CryptoRngCore, S: DigestSigner, C: TlsClock> CryptoProvider for Provider<'_, R, S, C> {
    type CipherSuite = Aes128GcmSha256;
    type Signature = Der;

    fn rng(&mut self) -> impl CryptoRngCore {
        &mut self.rng
    }

    fn verifier(&mut self) -> Result<&mut impl TlsVerifier<Aes128GcmSha256>, TlsError> {
        Ok(&mut self.verifier)
    }

    fn signer(&mut self) -> Result<(impl SignerMut<Der>, SignatureScheme), TlsError> {
        Ok((DeviceKey(&mut self.key), SignatureScheme::EcdsaSecp256r1Sha256))
    }

    fn client_cert(&mut self) -> Option<Certificate<impl AsRef<[u8]>>> {
        Some(Certificate::X509(self.cert))
    }
}

// ----------------------------------------------------------------------------- io

/// An I/O error from the connection under TLS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoError(Kind07);

impl core::fmt::Display for IoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl core::error::Error for IoError {}

impl embedded_io_07::Error for IoError {
    fn kind(&self) -> Kind07 {
        self.0
    }
}

fn kind(k: Kind06) -> Kind07 {
    match k {
        Kind06::NotFound => Kind07::NotFound,
        Kind06::PermissionDenied => Kind07::PermissionDenied,
        Kind06::ConnectionRefused => Kind07::ConnectionRefused,
        Kind06::ConnectionReset => Kind07::ConnectionReset,
        Kind06::ConnectionAborted => Kind07::ConnectionAborted,
        Kind06::NotConnected => Kind07::NotConnected,
        Kind06::AddrInUse => Kind07::AddrInUse,
        Kind06::AddrNotAvailable => Kind07::AddrNotAvailable,
        Kind06::BrokenPipe => Kind07::BrokenPipe,
        Kind06::AlreadyExists => Kind07::AlreadyExists,
        Kind06::InvalidInput => Kind07::InvalidInput,
        Kind06::InvalidData => Kind07::InvalidData,
        Kind06::TimedOut => Kind07::TimedOut,
        Kind06::Interrupted => Kind07::Interrupted,
        Kind06::Unsupported => Kind07::Unsupported,
        Kind06::OutOfMemory => Kind07::OutOfMemory,
        Kind06::WriteZero => Kind07::WriteZero,
        _ => Kind07::Other,
    }
}

/// A connection's two halves as the one socket `embedded-tls` takes: it is `Clone` (a copy of two references), and each copy
/// reads through the reader and writes through the writer, so a TLS reader and a TLS writer made from one connection never
/// touch the same half. Each half sits in an async `Mutex`, so a second user would wait, not corrupt.
pub struct Duplex<'a, R, W> {
    r: &'a Mutex<NoopRawMutex, R>,
    w: &'a Mutex<NoopRawMutex, W>,
}

impl<'a, R, W> Duplex<'a, R, W> {
    pub fn new(r: &'a Mutex<NoopRawMutex, R>, w: &'a Mutex<NoopRawMutex, W>) -> Self {
        Self { r, w }
    }
}

impl<R, W> Clone for Duplex<'_, R, W> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<R, W> Copy for Duplex<'_, R, W> {}

impl<R, W> ErrorType07 for Duplex<'_, R, W> {
    type Error = IoError;
}

impl<R: Read06, W> Read07 for Duplex<'_, R, W> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        self.r.lock().await.read(buf).await.map_err(|e| IoError(kind(e.kind())))
    }
}

impl<R, W: Write06> Write07 for Duplex<'_, R, W> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        self.w
            .lock()
            .await
            .write(buf)
            .await
            .map_err(|e| IoError(kind(e.kind())))
    }

    async fn flush(&mut self) -> Result<(), IoError> {
        self.w.lock().await.flush().await.map_err(|e| IoError(kind(e.kind())))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::io::{Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::string::String as Str;
    use std::vec;
    use std::vec::Vec as V;

    use embedded_io_async::ErrorType as ErrorType06;
    use embedded_tls::{NoClock, TlsConfig, TlsConnection, TlsContext};
    use rand_core::OsRng;

    use super::*;
    use crate::drivers::atecc608::{ADDRESS, Atecc608};
    use crate::drivers::cert;
    use crate::drivers::csr::{self, ToBeSigned};
    use crate::drivers::identity::tests::{Chip, blank};
    use crate::drivers::identity::{KEY_SLOT, provision};
    use crate::drivers::mock::{NoDelay, block_on};

    /// The model chip as the TLS signer.
    struct ChipKey(Atecc608<Chip, NoDelay>);
    impl DigestSigner for ChipKey {
        fn sign(&mut self, digest: &[u8; 32]) -> Result<[u8; 64], SignFailed> {
            block_on(self.0.sign(KEY_SLOT, digest)).map_err(|_| SignFailed)
        }
    }

    /// A std TCP stream as the connection halves a `Connector` gives (blocking inside the async calls: the test's server
    /// is another process, so a read just waits for it).
    struct Half(TcpStream);
    #[derive(Debug)]
    struct E(std::io::ErrorKind);
    impl embedded_io_async::Error for E {
        fn kind(&self) -> Kind06 {
            match self.0 {
                std::io::ErrorKind::UnexpectedEof => Kind06::ConnectionReset,
                _ => Kind06::Other,
            }
        }
    }
    impl ErrorType06 for Half {
        type Error = E;
    }
    impl Read06 for Half {
        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, E> {
            self.0.read(buf).map_err(|e| E(e.kind()))
        }
    }
    impl Write06 for Half {
        async fn write(&mut self, buf: &[u8]) -> Result<usize, E> {
            self.0.write(buf).map_err(|e| E(e.kind()))
        }
        async fn flush(&mut self) -> Result<(), E> {
            self.0.flush().map_err(|e| E(e.kind()))
        }
    }

    struct Server(Child);
    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn openssl(dir: &Path, args: &[&str]) -> Option<V<u8>> {
        let o = Command::new("openssl")
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        o.status.success().then_some(o.stdout)
    }

    fn pem_of(der: &[u8], label: &str) -> Str {
        let mut s = Str::new();
        for l in csr::pem::<12>(label, der).unwrap() {
            s.push_str(&l);
            s.push('\n');
        }
        s
    }

    struct Pki {
        dir: PathBuf,
        ca_der: V<u8>,
        client_der: V<u8>,
        chip: Atecc608<Chip, NoDelay>,
    }

    /// A CA, a server certificate for `localhost`, and a client certificate the CA signs for the chip's key, or `None`
    /// where there is no openssl.
    fn pki(name: &str) -> Option<Pki> {
        let dir = std::env::temp_dir().join(std::format!("mm-tls-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let o = |args: &[&str]| openssl(&dir, args);
        o(&["version"])?;
        o(&["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", "ca.key"])?;
        o(&[
            "req",
            "-x509",
            "-new",
            "-key",
            "ca.key",
            "-subj",
            "/CN=mm test ca",
            "-days",
            "30",
            "-out",
            "ca.pem",
        ])?;
        o(&[
            "ecparam",
            "-name",
            "prime256v1",
            "-genkey",
            "-noout",
            "-out",
            "server.key",
        ])?;
        o(&[
            "req",
            "-new",
            "-key",
            "server.key",
            "-subj",
            "/CN=localhost",
            "-out",
            "server.csr",
        ])?;
        std::fs::write(dir.join("ext.cnf"), "subjectAltName=DNS:localhost\n").ok()?;
        o(&[
            "x509",
            "-req",
            "-in",
            "server.csr",
            "-CA",
            "ca.pem",
            "-CAkey",
            "ca.key",
            "-CAcreateserial",
            "-days",
            "30",
            "-extfile",
            "ext.cnf",
            "-out",
            "server.pem",
        ])?;
        let ca_der = o(&["x509", "-in", "ca.pem", "-outform", "DER"])?;

        let mut chip = Atecc608::new(Chip::new(blank()), ADDRESS, NoDelay);
        let key = block_on(provision(&mut chip, true, &mut async |_: &str| {})).ok()??;
        let tbs = ToBeSigned::new("mm-a1b2c3", &key)?;
        let sig = block_on(chip.sign(KEY_SLOT, &tbs.digest())).ok()?;
        let request = tbs.finish(&sig)?;
        std::fs::write(dir.join("dev.csr"), pem_of(&request, "CERTIFICATE REQUEST")).ok()?;
        let client_der = o(&[
            "x509",
            "-req",
            "-in",
            "dev.csr",
            "-CA",
            "ca.pem",
            "-CAkey",
            "ca.key",
            "-CAcreateserial",
            "-days",
            "30",
            "-outform",
            "DER",
        ])?;
        // (as the board keeps it: in the chip's certificate slot)
        block_on(cert::store(&mut chip, &client_der)).ok()?;
        let client_der = block_on(cert::load(&mut chip)).ok()??.to_vec();
        Some(Pki {
            dir,
            ca_der,
            client_der,
            chip,
        })
    }

    /// `openssl s_server`, TLS 1.3 only, asking for a client certificate and answering a GET with a status page.
    fn serve(dir: &Path, require_client: bool) -> Option<(Server, u16)> {
        let port = TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok()?.port();
        let mut args = vec![
            "s_server",
            "-accept",
            "",
            "-tls1_3",
            "-cert",
            "server.pem",
            "-key",
            "server.key",
            "-CAfile",
            "ca.pem",
            "-www",
            "-quiet",
        ];
        let p = port.to_string();
        args[2] = &p;
        if require_client {
            args.extend(["-Verify", "1", "-verify_return_error"]);
        }
        let child = Command::new("openssl")
            .args(&args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let server = Server(child);
        for _ in 0..100 {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                // (that connection was a probe; the server ignores it)
                return Some((server, port));
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        None
    }

    /// Opens a TLS connection with `provider` to `port`, sends a GET and returns the start of the answer, or the error.
    fn get(port: u16, provider: Provider<'_, OsRng, ChipKey, NoClock>) -> Result<Str, TlsError> {
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let (r, w) = (Mutex::new(Half(stream.try_clone().unwrap())), Mutex::new(Half(stream)));
        let mut read = vec![0u8; RECORD];
        let mut write = vec![0u8; RECORD];
        let config = TlsConfig::new().with_server_name("localhost");
        let mut tls = TlsConnection::new(Duplex::new(&r, &w), &mut read, &mut write);
        block_on(tls.open(TlsContext::new(&config, provider)))?;
        block_on(async {
            tls.write(b"GET / HTTP/1.0\r\n\r\n").await?;
            tls.flush().await?;
            let mut answer = vec![0u8; 400];
            let mut n = 0;
            while n < 12 {
                let got = tls.read(&mut answer[n..]).await?;
                if got == 0 {
                    break;
                }
                n += got;
            }
            Ok(Str::from_utf8_lossy(&answer[..n]).into_owned())
        })
    }

    #[test]
    fn the_chip_signs_the_handshake_and_a_server_that_needs_a_client_certificate_lets_it_in() {
        let Some(pki) = pki("ok") else { return };
        let (_server, port) = serve(&pki.dir, true).expect("s_server");
        let provider = Provider::new(OsRng, &pki.ca_der, ChipKey(pki.chip), &pki.client_der);
        let answer = get(port, provider).expect("handshake and a request");
        assert!(answer.starts_with("HTTP/1.0 200"), "{answer}");
        let _ = std::fs::remove_dir_all(&pki.dir);
    }

    #[test]
    fn without_the_chip_key_the_server_turns_the_client_away() {
        let Some(pki) = pki("nokey") else { return };
        let (_server, port) = serve(&pki.dir, true).expect("s_server");
        // a chip that is not the one the certificate is for: its signature does not match the certificate's key
        let mut other = Atecc608::new(Chip::new(blank()), ADDRESS, NoDelay);
        block_on(provision(&mut other, true, &mut async |_: &str| {})).unwrap();
        // (the model makes the same key every time, so give it a different one by making a second)
        block_on(other.generate_key(KEY_SLOT)).unwrap_or([0; 64]);
        let provider = Provider::new(OsRng, &pki.ca_der, ChipKey(other), &pki.client_der);
        assert!(get(port, provider).is_err());
        let _ = std::fs::remove_dir_all(&pki.dir);
    }

    #[test]
    fn a_server_the_pinned_ca_did_not_sign_is_refused() {
        let Some(pki) = pki("pin") else { return };
        let (_server, port) = serve(&pki.dir, false).expect("s_server");
        // pin another CA
        let other = std::env::temp_dir().join(std::format!("mm-tls-other-{}", std::process::id()));
        std::fs::create_dir_all(&other).unwrap();
        openssl(
            &other,
            &["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", "o.key"],
        )
        .unwrap();
        openssl(
            &other,
            &[
                "req",
                "-x509",
                "-new",
                "-key",
                "o.key",
                "-subj",
                "/CN=someone else",
                "-days",
                "30",
                "-out",
                "o.pem",
            ],
        )
        .unwrap();
        let wrong_ca = openssl(&other, &["x509", "-in", "o.pem", "-outform", "DER"]).unwrap();
        let provider = Provider::new(OsRng, &wrong_ca, ChipKey(pki.chip), &pki.client_der);
        assert!(get(port, provider).is_err());
        let _ = std::fs::remove_dir_all(&pki.dir);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn the_signer_gives_a_der_signature_over_the_sha256_of_the_message() {
        use p256::ecdsa::signature::hazmat::PrehashVerifier;
        use p256::ecdsa::{Signature, VerifyingKey};
        let mut chip = Atecc608::new(Chip::new(blank()), ADDRESS, NoDelay);
        let key = block_on(provision(&mut chip, true, &mut async |_: &str| {}))
            .unwrap()
            .unwrap();
        let mut signer = ChipKey(chip);
        let sig = DeviceKey(&mut signer).try_sign(b"what TLS wants signed").unwrap();
        let sig = Signature::from_der(&sig).expect("DER");
        let mut sec1 = vec![4u8];
        sec1.extend_from_slice(&key);
        let vk = VerifyingKey::from_sec1_bytes(&sec1).unwrap();
        let digest: [u8; 32] = Sha256::digest(b"what TLS wants signed").into();
        vk.verify_prehash(&digest, &sig).expect("verifies");
    }
}
