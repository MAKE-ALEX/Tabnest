//! 出站请求的 SSRF 防护所使用的地址分类逻辑。

use std::net::{IpAddr, Ipv4Addr};

/// 判断一个 IP 是否禁止作为出站抓取目标。
///
/// 拒绝回环、私有、链路本地、CGNAT、组播、文档示例等非公网地址；
/// IPv4 映射的 IPv6 地址（`::ffff:a.b.c.d`）会先拆出 IPv4 再判断，
/// 防止通过这种写法绕过 IPv4 规则。
pub fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_forbidden_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_forbidden_v4(v4);
            }
            let segments = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // fc00::/7：IPv6 唯一本地地址（类似内网）。
                || (segments[0] & 0xfe00) == 0xfc00
                // fe80::/10：链路本地地址。
                || (segments[0] & 0xffc0) == 0xfe80
                // 2001:db8::/32：文档示例地址。
                || (segments[0] == 0x2001 && segments[1] == 0x0db8)
                // 2002::/16：6to4 隧道地址。
                || segments[0] == 0x2002
                // 2001::/32：Teredo 隧道地址。
                || (segments[0] == 0x2001 && segments[1] == 0x0000)
        }
    }
}

/// IPv4 版本的禁用网段判断。
fn is_forbidden_v4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        // 0.0.0.0/8：本网络。
        || octets[0] == 0
        // 100.64.0.0/10：运营商级 NAT（CGNAT）。
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        // 192.0.0.0/24：IETF 协议分配。
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        // 192.88.99.0/24：6to4 中继。
        || (octets[0] == 192 && octets[1] == 88 && octets[2] == 99)
        // 198.18.0.0/15：基准测试网段。
        || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
        // 240.0.0.0/4：保留地址（含 255.255.255.255 已单独判断）。
        || octets[0] >= 240
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_private_and_special_addresses() {
        for raw in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.20.110",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "198.18.0.1",
            "::1",
            "::",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.1.1",
        ] {
            let ip: IpAddr = raw.parse().unwrap();
            assert!(is_forbidden_ip(ip), "{raw} should be blocked");
        }
    }

    #[test]
    fn allows_public_addresses() {
        for raw in ["1.1.1.1", "8.8.8.8", "93.184.216.34", "2606:4700:4700::1111"] {
            let ip: IpAddr = raw.parse().unwrap();
            assert!(!is_forbidden_ip(ip), "{raw} should be allowed");
        }
    }
}
