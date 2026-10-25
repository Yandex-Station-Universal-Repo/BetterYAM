//! The wire side of the setup access point: a DHCP server that hands phones
//! an address and names the station as their router and DNS, and a DNS
//! server that answers every name with the station, so a phone's "is there
//! internet?" probe lands on the setup page and it shows "sign in to the
//! network". Pure functions over packets; the sockets live in `wifi_setup`.
//! Built in, so the setup mode needs no dnsmasq or udhcpd on the station
//! (the Mini firmware ships neither).

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

pub const LEASE: Duration = Duration::from_secs(3600);
const COOKIE: [u8; 4] = [99, 130, 83, 99];

const DISCOVER: u8 = 1;
const OFFER: u8 = 2;
const REQUEST: u8 = 3;
const ACK: u8 = 5;
const NAK: u8 = 6;
const RELEASE: u8 = 7;

/// The access point's network: the station at `server`, phones from
/// `first` to `last`, a /24.
#[derive(Debug, Clone, Copy)]
pub struct Subnet {
    pub server: Ipv4Addr,
    pub first: u8,
    pub last: u8,
}

impl Subnet {
    fn host(&self, n: u8) -> Ipv4Addr {
        let o = self.server.octets();
        Ipv4Addr::new(o[0], o[1], o[2], n)
    }

    fn contains(&self, ip: Ipv4Addr) -> bool {
        let (a, b) = (ip.octets(), self.server.octets());
        a[..3] == b[..3] && (self.first..=self.last).contains(&a[3])
    }
}

/// Who has which address, and since when it was last asked for.
#[derive(Debug, Default)]
pub struct Leases {
    by_mac: HashMap<[u8; 6], (Ipv4Addr, Instant)>,
}

impl Leases {
    /// The address this phone has, or the first free one.
    fn pick(&self, subnet: &Subnet, mac: [u8; 6], now: Instant) -> Option<Ipv4Addr> {
        if let Some((ip, _)) = self.by_mac.get(&mac) {
            return Some(*ip);
        }
        (subnet.first..=subnet.last)
            .map(|n| subnet.host(n))
            .find(|ip| !self.taken_by_other(*ip, mac, now))
    }

    fn taken_by_other(&self, ip: Ipv4Addr, mac: [u8; 6], now: Instant) -> bool {
        self.by_mac
            .iter()
            .any(|(m, (i, at))| *i == ip && *m != mac && now.duration_since(*at) < LEASE)
    }

    /// Phones that asked for an address within `within`.
    pub fn recent(&self, now: Instant, within: Duration) -> usize {
        self.by_mac
            .values()
            .filter(|(_, at)| now.duration_since(*at) < within)
            .count()
    }
}

/// A DHCP request in, the reply to send (broadcast to port 68), if any.
pub fn dhcp_reply(
    subnet: &Subnet,
    leases: &mut Leases,
    packet: &[u8],
    now: Instant,
) -> Option<Vec<u8>> {
    if packet.len() < 240
        || packet[0] != 1
        || packet[1] != 1
        || packet[2] != 6
        || packet[236..240] != COOKIE
    {
        return None;
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&packet[28..34]);
    let options = parse_options(&packet[240..]);
    let kind = *options.get(&53)?.first()?;
    let requested = options
        .get(&50)
        .filter(|v| v.len() == 4)
        .map(|v| Ipv4Addr::new(v[0], v[1], v[2], v[3]))
        .or_else(|| {
            Some(Ipv4Addr::new(
                packet[12], packet[13], packet[14], packet[15],
            ))
            .filter(|ip| !ip.is_unspecified())
        });
    let server_id = options
        .get(&54)
        .filter(|v| v.len() == 4)
        .map(|v| Ipv4Addr::new(v[0], v[1], v[2], v[3]));
    match kind {
        DISCOVER => {
            let ip = leases.pick(subnet, mac, now)?;
            Some(build_reply(subnet, packet, OFFER, ip))
        }
        REQUEST => {
            // a request meant for another server is not ours to answer
            if server_id.is_some_and(|id| id != subnet.server) {
                return None;
            }
            let ip = requested?;
            if subnet.contains(ip) && !leases.taken_by_other(ip, mac, now) {
                leases.by_mac.insert(mac, (ip, now));
                Some(build_reply(subnet, packet, ACK, ip))
            } else {
                Some(build_reply(subnet, packet, NAK, Ipv4Addr::UNSPECIFIED))
            }
        }
        RELEASE => {
            leases.by_mac.remove(&mac);
            None
        }
        _ => None,
    }
}

fn parse_options(mut data: &[u8]) -> HashMap<u8, Vec<u8>> {
    let mut out = HashMap::new();
    while let Some((&code, rest)) = data.split_first() {
        match code {
            0 => data = rest,
            255 => break,
            _ => {
                let Some((&len, rest)) = rest.split_first() else {
                    break;
                };
                let len = len as usize;
                if rest.len() < len {
                    break;
                }
                out.insert(code, rest[..len].to_vec());
                data = &rest[len..];
            }
        }
    }
    out
}

fn build_reply(subnet: &Subnet, request: &[u8], kind: u8, yiaddr: Ipv4Addr) -> Vec<u8> {
    let mut p = vec![0u8; 240];
    p[0] = 2; // BOOTREPLY
    p[1] = 1;
    p[2] = 6;
    p[4..8].copy_from_slice(&request[4..8]); // xid
    p[10..12].copy_from_slice(&request[10..12]); // flags
    p[16..20].copy_from_slice(&yiaddr.octets());
    if kind != NAK {
        p[20..24].copy_from_slice(&subnet.server.octets());
    }
    p[28..44].copy_from_slice(&request[28..44]); // chaddr
    p[236..240].copy_from_slice(&COOKIE);
    let server = subnet.server.octets();
    let mut opt = |code: u8, value: &[u8]| {
        p.push(code);
        p.push(value.len() as u8);
        p.extend_from_slice(value);
    };
    opt(53, &[kind]);
    opt(54, &server);
    if kind != NAK {
        let secs = |d: u64| (d as u32).to_be_bytes();
        opt(51, &secs(LEASE.as_secs()));
        opt(58, &secs(LEASE.as_secs() / 2));
        opt(59, &secs(LEASE.as_secs() * 7 / 8));
        opt(1, &[255, 255, 255, 0]);
        opt(3, &server);
        opt(6, &server);
    }
    p.push(255);
    // some clients drop replies shorter than a BOOTP packet
    if p.len() < 300 {
        p.resize(300, 0);
    }
    p
}

/// A DNS query in, the answer: every A question gets `ip`, other types an
/// empty answer. None for what is not a query.
pub fn dns_reply(query: &[u8], ip: Ipv4Addr) -> Option<Vec<u8>> {
    if query.len() < 12 || query[2] & 0x80 != 0 || u16::from_be_bytes([query[4], query[5]]) != 1 {
        return None;
    }
    // the question: labels up to the root, then type and class
    let mut i = 12;
    loop {
        let len = *query.get(i)? as usize;
        if len & 0xc0 != 0 {
            return None;
        }
        i += 1 + len;
        if len == 0 {
            break;
        }
    }
    let end = i + 4;
    let question = query.get(12..end)?;
    let qtype = u16::from_be_bytes([query[i], query[i + 1]]);
    let answer = qtype == 1 || qtype == 255;
    let mut r = Vec::with_capacity(end + 16);
    r.extend_from_slice(&query[0..2]);
    // a response, recursion desired as asked, recursion available
    r.extend_from_slice(&[0x80 | (query[2] & 0x01), 0x80]);
    r.extend_from_slice(&[0, 1, 0, answer as u8, 0, 0, 0, 0]);
    r.extend_from_slice(question);
    if answer {
        r.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
        r.extend_from_slice(&ip.octets());
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NET: Subnet = Subnet {
        server: Ipv4Addr::new(192, 168, 4, 1),
        first: 10,
        last: 12,
    };

    fn request(kind: u8, mac: u8, extra: &[(u8, &[u8])]) -> Vec<u8> {
        let mut p = vec![0u8; 240];
        p[0] = 1;
        p[1] = 1;
        p[2] = 6;
        p[4..8].copy_from_slice(&[1, 2, 3, 4]);
        p[28..34].copy_from_slice(&[2, 0, 0, 0, 0, mac]);
        p[236..240].copy_from_slice(&COOKIE);
        p.extend_from_slice(&[53, 1, kind]);
        for (code, value) in extra {
            p.push(*code);
            p.push(value.len() as u8);
            p.extend_from_slice(value);
        }
        p.push(255);
        p
    }

    fn kind_and_ip(reply: &[u8]) -> (u8, Ipv4Addr) {
        let o = parse_options(&reply[240..]);
        (
            o[&53][0],
            Ipv4Addr::new(reply[16], reply[17], reply[18], reply[19]),
        )
    }

    #[test]
    fn discover_offer_request_ack() {
        let mut leases = Leases::default();
        let now = Instant::now();
        let offer = dhcp_reply(&NET, &mut leases, &request(DISCOVER, 1, &[]), now).unwrap();
        assert_eq!(kind_and_ip(&offer), (OFFER, Ipv4Addr::new(192, 168, 4, 10)));
        assert_eq!(&offer[4..8], &[1, 2, 3, 4]);
        let o = parse_options(&offer[240..]);
        assert_eq!(o[&3], [192, 168, 4, 1]);
        assert_eq!(o[&6], [192, 168, 4, 1]);
        let req = request(
            REQUEST,
            1,
            &[(50, &[192, 168, 4, 10]), (54, &[192, 168, 4, 1])],
        );
        let ack = dhcp_reply(&NET, &mut leases, &req, now).unwrap();
        assert_eq!(kind_and_ip(&ack), (ACK, Ipv4Addr::new(192, 168, 4, 10)));
        assert_eq!(leases.recent(now, Duration::from_secs(60)), 1);
        // a second phone gets the next address
        let offer2 = dhcp_reply(&NET, &mut leases, &request(DISCOVER, 2, &[]), now).unwrap();
        assert_eq!(kind_and_ip(&offer2).1, Ipv4Addr::new(192, 168, 4, 11));
    }

    #[test]
    fn foreign_or_taken_requests() {
        let mut leases = Leases::default();
        let now = Instant::now();
        // meant for another DHCP server: silence
        let other = request(
            REQUEST,
            1,
            &[(50, &[192, 168, 4, 10]), (54, &[10, 0, 0, 1])],
        );
        assert!(dhcp_reply(&NET, &mut leases, &other, now).is_none());
        // an address from another network: NAK
        let wrong = request(REQUEST, 1, &[(50, &[10, 0, 0, 5])]);
        assert_eq!(
            kind_and_ip(&dhcp_reply(&NET, &mut leases, &wrong, now).unwrap()).0,
            NAK
        );
        // an address another phone holds: NAK
        dhcp_reply(
            &NET,
            &mut leases,
            &request(REQUEST, 1, &[(50, &[192, 168, 4, 10])]),
            now,
        )
        .unwrap();
        let taken = request(REQUEST, 2, &[(50, &[192, 168, 4, 10])]);
        assert_eq!(
            kind_and_ip(&dhcp_reply(&NET, &mut leases, &taken, now).unwrap()).0,
            NAK
        );
    }

    #[test]
    fn full_pool_offers_nothing() {
        let mut leases = Leases::default();
        let now = Instant::now();
        for mac in 1..=3 {
            let ip = [192, 168, 4, 9 + mac];
            dhcp_reply(&NET, &mut leases, &request(REQUEST, mac, &[(50, &ip)]), now).unwrap();
        }
        assert!(dhcp_reply(&NET, &mut leases, &request(DISCOVER, 9, &[]), now).is_none());
    }

    #[test]
    fn junk_is_ignored() {
        let mut leases = Leases::default();
        assert!(dhcp_reply(&NET, &mut leases, &[0u8; 10], Instant::now()).is_none());
        assert!(dns_reply(&[0u8; 5], NET.server).is_none());
    }

    fn query(name: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0xab, 0xcd, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&[0, 1]);
        q
    }

    #[test]
    fn every_name_is_the_station() {
        let q = query("connectivitycheck.gstatic.com", 1);
        let r = dns_reply(&q, NET.server).unwrap();
        assert_eq!(&r[0..2], &[0xab, 0xcd]);
        assert_eq!(r[2] & 0x80, 0x80);
        assert_eq!(u16::from_be_bytes([r[6], r[7]]), 1);
        assert_eq!(&r[r.len() - 4..], &[192, 168, 4, 1]);
        // AAAA: an empty answer, so the phone falls back to the A record
        let r6 = dns_reply(&query("captive.apple.com", 28), NET.server).unwrap();
        assert_eq!(u16::from_be_bytes([r6[6], r6[7]]), 0);
        assert_eq!(
            r6.len(),
            q.len() - "connectivitycheck.gstatic.com".len() + "captive.apple.com".len()
        );
    }
}
