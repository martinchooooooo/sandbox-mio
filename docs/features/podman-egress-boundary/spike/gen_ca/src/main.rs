//! Spike for docs/features/podman-egress-boundary/02_dev.md, Resolved Decision 11:
//! an rcgen ECDSA P-256 CA with ten-year validity, written as ca.pem and ca-key.pem.
use rcgen::{BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose};

fn main() {
    let out = std::env::args().nth(1).expect("usage: gen_ca <out dir>");
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Sandbox-mio proxy CA (spike)");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(3650);
    let cert = params.self_signed(&key).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(format!("{out}/ca.pem"), cert.pem()).unwrap();
    std::fs::write(format!("{out}/ca-key.pem"), key.serialize_pem()).unwrap();
    println!("wrote {out}/ca.pem and {out}/ca-key.pem");
}
