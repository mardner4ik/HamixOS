use wlan::crypto::*;
use wlan::frame::*;
use wlan::{Radio, State, Station};

const AP: [u8; 6] = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
const STA: [u8; 6] = [0x02, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE];

#[derive(Default)]
struct Air {
    sent: Vec<Vec<u8>>,
    channel: u8,
}

impl Radio for Air {
    fn mac(&self) -> [u8; 6] {
        STA
    }
    fn set_channel(&mut self, channel: u8) -> bool {
        self.channel = channel;
        true
    }
    fn set_bssid(&mut self, _: Option<[u8; 6]>) {}
    fn transmit(&mut self, frame: &[u8], _: bool) -> bool {
        self.sent.push(frame.to_vec());
        true
    }
}

fn beacon() -> Vec<u8> {
    let mut f = Vec::new();
    build_header(&mut f, TYPE_MGMT, SUB_BEACON, 0, BROADCAST, AP, AP, 1);
    f.extend_from_slice(&[0u8; 8]);
    f.extend_from_slice(&100u16.to_le_bytes());
    f.extend_from_slice(&0x0411u16.to_le_bytes());
    f.extend_from_slice(&[IE_SSID, 6]);
    f.extend_from_slice(b"HomeAP");
    f.extend_from_slice(&[IE_DS, 1, 6]);
    f.push(IE_RSN);
    f.push(RSN_IE_CCMP_PSK.len() as u8);
    f.extend_from_slice(&RSN_IE_CCMP_PSK);
    f
}

fn mgmt(subtype: u8, body: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    build_header(&mut f, TYPE_MGMT, subtype, 0, STA, AP, AP, 2);
    f.extend_from_slice(body);
    f
}

fn eapol(key_info: u16, replay: u64, nonce: &[u8; 32], key_data: &[u8], kck: Option<&[u8; 16]>) -> Vec<u8> {
    let mut e = vec![0x02, 0x03];
    e.extend_from_slice(&((95 + key_data.len()) as u16).to_be_bytes());
    e.push(2);
    e.extend_from_slice(&key_info.to_be_bytes());
    e.extend_from_slice(&16u16.to_be_bytes());
    e.extend_from_slice(&replay.to_be_bytes());
    e.extend_from_slice(nonce);
    e.extend_from_slice(&[0u8; 48]);
    e.extend_from_slice(&(key_data.len() as u16).to_be_bytes());
    e.extend_from_slice(key_data);
    if let Some(k) = kck {
        let mic = hmac_sha1(k, &[&e]);
        e[81..97].copy_from_slice(&mic[..16]);
    }
    e
}

fn data_from_ap(payload: &[u8], protected: Option<(&[u8; 16], u64)>) -> Vec<u8> {
    let mut f = Vec::new();
    build_header(&mut f, TYPE_DATA, 0, FC_FROM_DS | if protected.is_some() { FC_PROTECTED } else { 0 }, STA, AP, AP, 5);
    match protected {
        None => f.extend_from_slice(payload),
        Some((tk, pn)) => {
            let (nonce, aad) = wlan::ccmp_nonce_aad(&f[..24], pn);
            let b = pn.to_le_bytes();
            f.extend_from_slice(&[b[0], b[1], 0, 0x20, b[2], b[3], b[4], b[5]]);
            f.extend_from_slice(&ccm_encrypt(tk, &nonce, &aad, payload));
        }
    }
    f
}

#[test]
fn wpa2_connect_and_exchange() {
    let mut air = Air::default();
    let mut sta = Station::new(0x1234_5678);
    sta.enable(true, &mut air);
    let mut now = 0;
    sta.on_frame(&beacon(), -40, now, &mut air);
    while sta.state == State::Scanning {
        now += 50;
        sta.tick(now, &mut air);
    }
    assert_eq!(sta.networks().len(), 1);
    sta.connect("HomeAP", "correct horse battery", now, &mut air).unwrap();
    assert_eq!(sta.state, State::Authenticating);
    assert_eq!(air.channel, 6);
    sta.on_frame(&mgmt(SUB_AUTH, &[0, 0, 2, 0, 0, 0]), -40, now, &mut air);
    assert_eq!(sta.state, State::Associating);
    let assoc_req = air.sent.last().unwrap().clone();
    assert!(parse(&assoc_req).map(|h| h.subtype == SUB_ASSOC_REQ).unwrap());
    sta.on_frame(&mgmt(SUB_ASSOC_RESP, &[0x11, 0x04, 0, 0, 1, 0xC0]), -40, now, &mut air);
    assert_eq!(sta.state, State::Handshake);

    let pmk = psk_from_passphrase("correct horse battery", b"HomeAP");
    let anonce = [0x5Au8; 32];
    let mut llc = vec![0xAA, 0xAA, 0x03, 0, 0, 0, 0x88, 0x8E];
    llc.extend_from_slice(&eapol(0x008A, 1, &anonce, &[], None));
    sta.on_frame(&data_from_ap(&llc, None), -40, now, &mut air);
    let msg2 = air.sent.last().unwrap().clone();
    let h = parse(&msg2).unwrap();
    assert_eq!(h.flags & FC_PROTECTED, 0);
    let e = &h.body[8..];
    let snonce: [u8; 32] = e[17..49].try_into().unwrap();
    let (min_mac, max_mac) = if STA < AP { (STA, AP) } else { (AP, STA) };
    let (a, b) = if snonce < anonce { (snonce, anonce) } else { (anonce, snonce) };
    let mut seed = Vec::new();
    seed.extend_from_slice(&min_mac);
    seed.extend_from_slice(&max_mac);
    seed.extend_from_slice(&a);
    seed.extend_from_slice(&b);
    let mut ptk = [0u8; 48];
    prf_80211(&pmk, b"Pairwise key expansion", &seed, &mut ptk);
    let kck: [u8; 16] = ptk[0..16].try_into().unwrap();
    let kek: [u8; 16] = ptk[16..32].try_into().unwrap();
    let tk: [u8; 16] = ptk[32..48].try_into().unwrap();
    let mut check = e.to_vec();
    let mic: Vec<u8> = check[81..97].to_vec();
    check[81..97].fill(0);
    assert_eq!(hmac_sha1(&kck, &[&check])[..16].to_vec(), mic, "message 2 MIC");

    let gtk = [0x77u8; 16];
    let mut kd = vec![IE_RSN, RSN_IE_CCMP_PSK.len() as u8];
    kd.extend_from_slice(&RSN_IE_CCMP_PSK);
    kd.extend_from_slice(&[0xDD, 22, 0x00, 0x0F, 0xAC, 0x01, 0x01, 0x00]);
    kd.extend_from_slice(&gtk);
    while kd.len() % 8 != 0 || kd.len() < 16 {
        kd.push(if kd.len() == 20 + 22 + 2 { 0xDD } else { 0 });
    }
    let wrapped = aes_wrap(&kek, &kd);
    let mut llc3 = vec![0xAA, 0xAA, 0x03, 0, 0, 0, 0x88, 0x8E];
    llc3.extend_from_slice(&eapol(0x13CA, 2, &anonce, &wrapped, Some(&kck)));
    sta.on_frame(&data_from_ap(&llc3, None), -40, now, &mut air);
    assert_eq!(sta.state, State::Connected, "{}", sta.message);

    let mut ip = vec![0xAA, 0xAA, 0x03, 0, 0, 0, 0x08, 0x00];
    ip.extend_from_slice(b"hello from the access point");
    sta.on_frame(&data_from_ap(&ip, Some((&tk, 1))), -40, now, &mut air);
    let eth = sta.take_ethernet().expect("decrypted frame");
    assert_eq!(&eth[0..6], &STA);
    assert_eq!(&eth[12..14], &[0x08, 0x00]);
    assert_eq!(&eth[14..], b"hello from the access point");

    let mut out = Vec::new();
    out.extend_from_slice(&[0xFF; 6]);
    out.extend_from_slice(&STA);
    out.extend_from_slice(&[0x08, 0x06]);
    out.extend_from_slice(b"arp payload");
    assert!(sta.send_ethernet(&out, &mut air));
    let sent = air.sent.last().unwrap().clone();
    let h = parse(&sent).unwrap();
    assert_ne!(h.flags & FC_PROTECTED, 0);
    let pn = u64::from_le_bytes([h.body[0], h.body[1], h.body[4], h.body[5], h.body[6], h.body[7], 0, 0]);
    let (nonce, aad) = wlan::ccmp_nonce_aad(&sent[..24], pn);
    let plain = ccm_decrypt(&tk, &nonce, &aad, &h.body[8..]).expect("AP can decrypt");
    assert_eq!(&plain[6..8], &[0x08, 0x06]);
    assert_eq!(&plain[8..], b"arp payload");
}
