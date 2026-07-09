use std::io::{self, Read, Write};
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore, SignatureScheme,
};

pub struct Tls {
    conn: ClientConnection,
}

impl Tls {
    pub fn client(host: &str, alpn: &[&[u8]]) -> io::Result<Self> {
        let name = ServerName::try_from(host.to_owned())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let mut config = if std::env::var_os("TINYNET_INSECURE_TLS").is_some() {
            static WARNING: std::sync::Once = std::sync::Once::new();
            WARNING.call_once(|| eprintln!("tinynet: TLS certificate verification is disabled"));
            ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(InsecureVerifier))
                .with_no_client_auth()
        } else {
            ClientConfig::builder()
                .with_root_certificates(native_roots()?)
                .with_no_client_auth()
        };
        config.alpn_protocols = alpn.iter().map(|protocol| protocol.to_vec()).collect();
        let conn = ClientConnection::new(Arc::new(config), name)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Tls { conn })
    }

    pub fn is_handshaking(&self) -> bool {
        self.conn.is_handshaking()
    }

    pub fn read_tls(&mut self, data: &[u8]) -> io::Result<()> {
        let mut remaining = data;
        while !remaining.is_empty() {
            let read = self.conn.read_tls(&mut remaining)?;
            if read == 0 {
                break;
            }
        }
        self.conn
            .process_new_packets()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(())
    }

    pub fn write_tls(&mut self, out: &mut Vec<u8>) -> io::Result<()> {
        while self.conn.wants_write() {
            self.conn.write_tls(out)?;
        }
        Ok(())
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.conn.reader().read(buf)
    }

    pub fn read_eof(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.conn.read_tls(&mut io::empty())?;
        self.conn
            .process_new_packets()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        accept_unclean_eof(self.read(buf))
    }

    pub fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.conn.writer().write(&data[..data.len().min(64 * 1024)])
    }
}

fn accept_unclean_eof(result: io::Result<usize>) -> io::Result<usize> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(0),
        result => result,
    }
}

#[derive(Debug)]
struct InsecureVerifier;

impl ServerCertVerifier for InsecureVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ED25519,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

fn native_roots() -> io::Result<RootCertStore> {
    let certs = rustls_native_certs::load_native_certs();
    if certs.certs.is_empty() && !certs.errors.is_empty() {
        return Err(io::Error::other(format!(
            "failed to load native root certificates: {:?}",
            certs.errors
        )));
    }

    let mut roots = RootCertStore::empty();
    for cert in certs.certs {
        roots.add(cert).map_err(io::Error::other)?;
    }
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::accept_unclean_eof;

    #[test]
    fn bare_tcp_eof_becomes_tls_eof() {
        assert_eq!(
            accept_unclean_eof(Err(io::Error::from(io::ErrorKind::UnexpectedEof))).unwrap(),
            0
        );
        assert_eq!(
            accept_unclean_eof(Err(io::Error::from(io::ErrorKind::InvalidData)))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
