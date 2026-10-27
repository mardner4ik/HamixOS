use wlan::crypto::*;

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

#[test]
fn sha1_abc() {
    assert_eq!(sha1(b"abc").to_vec(), hex("a9993e364706816aba3e25717850c26c9cd0d89d"));
    let long = vec![b'a'; 1_000_000];
    assert_eq!(sha1(&long).to_vec(), hex("34aa973cd4c4daa4f61eeb2bdbad27316534016f"));
}

#[test]
fn hmac_rfc2202() {
    assert_eq!(hmac_sha1(&[0x0b; 20], &[b"Hi There"]).to_vec(), hex("b617318655057264e28bc0b6fb378c8ef146be00"));
}

#[test]
fn psk_ieee() {
    assert_eq!(psk_from_passphrase("password", b"IEEE").to_vec(), hex("f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e"));
    assert_eq!(psk_from_passphrase("ThisIsAPassword", b"ThisIsASSID").to_vec(), hex("0dc0d6eb90555ed6419756b9a15ec3e3209b63df707dd508d14581f8982721af"));
}

#[test]
fn prf_ieee() {
    let mut out = [0u8; 64];
    prf_80211(&[0x0b; 20], b"prefix", b"Hi There", &mut out);
    assert_eq!(out.to_vec(), hex("bcd4c650b30b9684951829e0d75f9d54b862175ed9f00606e17d8da35402ffee75df78c3d31e0f889f012120c0862beb67753e7439ae242edb8373698356cf5a"));
}

#[test]
fn aes_fips197() {
    let key: [u8; 16] = hex("000102030405060708090a0b0c0d0e0f").try_into().unwrap();
    let aes = Aes128::new(&key);
    let mut block: [u8; 16] = hex("00112233445566778899aabbccddeeff").try_into().unwrap();
    aes.encrypt_block(&mut block);
    assert_eq!(block.to_vec(), hex("69c4e0d86a7b0430d8cdb78070b4c55a"));
    aes.decrypt_block(&mut block);
    assert_eq!(block.to_vec(), hex("00112233445566778899aabbccddeeff"));
}

#[test]
fn keywrap_rfc3394() {
    let kek: [u8; 16] = hex("000102030405060708090A0B0C0D0E0F").try_into().unwrap();
    let plain = hex("00112233445566778899AABBCCDDEEFF");
    let wrapped = aes_wrap(&kek, &plain);
    assert_eq!(wrapped, hex("1FA68B0A8112B447AEF34BD8FB5A7B829D3E862371D2CFE5"));
    assert_eq!(aes_unwrap(&kek, &wrapped).unwrap(), plain);
}

#[test]
fn ccm_rfc3610_packet1() {
    let key: [u8; 16] = hex("C0C1C2C3C4C5C6C7C8C9CACBCCCDCECF").try_into().unwrap();
    let nonce: [u8; 13] = hex("00000003020100A0A1A2A3A4A5").try_into().unwrap();
    let aad = hex("0001020304050607");
    let payload = hex("08090A0B0C0D0E0F101112131415161718191A1B1C1D1E");
    let out = ccm_encrypt(&key, &nonce, &aad, &payload);
    assert_eq!(out, hex("588C979A61C663D2F066D0C2C0F989806D5F6B61DAC38417E8D12CFDF926E0"));
    assert_eq!(ccm_decrypt(&key, &nonce, &aad, &out).unwrap(), payload);
}

#[test]
fn ccmp_ieee_example() {
    let tk: [u8; 16] = hex("c97c1f67ce371185514a8a19f2bdd52f").try_into().unwrap();
    let mpdu_header = hex("08480000 0f d2 e1 28 a5 7c 50 30 f1 84 44 08 ab ae a5 b8 fc ba 80 33");
    let pn = 0xB5039776E70Cu64;
    let plain = hex("f8ba1a55d02f85ae967bb62fb6cda8eb7e78a050");
    let (nonce, aad) = wlan::ccmp_nonce_aad(&mpdu_header, pn);
    assert_eq!(nonce.to_vec(), hex("0050 30f1 8444 08b5 0397 76e7 0c"));
    assert_eq!(aad, hex("0840 0fd2 e128 a57c 5030 f184 4408 abae a5b8 fcba 0000"));
    let out = ccm_encrypt(&tk, &nonce, &aad, &plain);
    assert_eq!(out, hex("f3d0a2fe9a3dbf2342a643e43246e80c3c04d019 7845ce0b16f97623"));
}
