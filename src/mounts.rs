//! Mount polling — replaces `gio::VolumeMonitor` with a pure-Rust
//! `/proc/mounts` reader. No GLib, no udev, no DBus: just the kernel's
//! own view of what's mounted where, polled on a timer.
//!
//! We filter out kernel pseudo-filesystems (proc, sysfs, tmpfs, cgroup,
//! etc.) so the sidebar shows only real storage. Network filesystems
//! (nfs, cifs, sshfs) are tagged so the UI can group them separately
//! if desired.
//!
//! The parser is split into a pure `parse_mounts(content: &str)` function
//! that takes the file contents as input, plus a thin `poll_mounts()`
//! wrapper that reads `/proc/mounts` and delegates. This makes the
//! parsing logic unit-testable without mocking the filesystem.

use std::path::{Path, PathBuf};

/// A single mount entry shown in the sidebar's "Devices" section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountInfo {
    /// Device source (e.g. `/dev/sda1`, `tmpfs`, `server:/export`).
    pub source: String,
    /// Where it's mounted (`/`, `/mnt/usb`, `/home`, ...).
    pub target: PathBuf,
    /// Filesystem type (`ext4`, `btrfs`, `nfs4`, ...).
    pub fs_type: String,
    /// True for nfs/cifs/sshfs/etc. — used for icon + grouping.
    pub is_network: bool,
    /// Display label: target basename, or "/" for root.
    pub label: String,
}

/// Pseudo-filesystems hidden from the sidebar. Sourced from
/// `man 5 filesystems` and common kernel-internal mounts. The sidebar
/// shows real storage, not the kernel's bookkeeping.
///
/// `fuse.gvfsd-fuse` lives here only. It is the GVFS userspace daemon's
/// fuse bridge, not a network filesystem in its own right. The network
/// mounts GVFS exposes (smb, sftp, dav, etc.) surface via FUSE and do
/// not carry distinct `fs_type` strings we can match on, so they are
/// hidden along with the rest of FUSE. Mount GVFS network shares via
/// udisks2 or fstab to produce real `cifs`/`nfs` entries we can tag.
const PSEUDO_FS: &[&str] = &[
    "proc", "sysfs", "tmpfs", "devtmpfs", "devpts", "cgroup", "cgroup2",
    "pstore", "bpf", "tracefs", "debugfs", "fusectl", "configfs",
    "securityfs", "mqueue", "hugetlbfs", "ramfs", "autofs", "binfmt_misc",
    "rpc_pipefs", "nsfs", "fuse.gvfsd-fuse", "fuse.snapfuse", "fuse",
    "efivarfs", "selinuxfs", "systemd-1", "none",
];

/// Network filesystems — surfaced separately so users can spot them.
/// `fuse.gvfsd-fuse` is excluded (see PSEUDO_FS above); the pseudo-fs
/// filter runs first, so listing it here would be dead code.
const NETWORK_FS: &[&str] = &[
    "nfs", "nfs4", "cifs", "smbfs", "smb2", "smb3", "sshfs", "fuse.sshfs",
];

/// Read `/proc/mounts` and parse it. Returns an empty vec on any error
/// (e.g. non-Linux) — we never panic on mount lookup failures.
pub fn poll_mounts() -> Vec<MountInfo> {
    let content = match std::fs::read_to_string("/proc/mounts") {
        Ok(s) => s,
        Err(e) => {
            log::debug!("could not read /proc/mounts: {e}");
            return Vec::new();
        }
    };
    parse_mounts(&content)
}

/// Pure parser: given the text contents of `/proc/mounts`, return the
/// list of real (non-pseudo) mounts. Sorted with root first, then
/// alphabetically by label.
///
/// Exposed publicly so the parsing logic can be unit-tested without
/// mocking `/proc/mounts`.
pub fn parse_mounts(content: &str) -> Vec<MountInfo> {
    let mut out = Vec::new();
    let mut seen_targets: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let source = unescape_octal(parts[0]);
        let target_raw = unescape_octal(parts[1]);
        let fs_type = parts[2].to_string();

        if PSEUDO_FS.iter().any(|p| *p == fs_type) {
            continue;
        }

        // Skip duplicate mount points (overlay-fs on top of /, etc.)
        if !seen_targets.insert(target_raw.clone()) {
            continue;
        }

        let target = PathBuf::from(&target_raw);
        let is_network = NETWORK_FS.iter().any(|p| *p == fs_type);

        let label = if target == Path::new("/") {
            "Root".to_string()
        } else {
            target
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| target_raw.clone())
        };

        out.push(MountInfo {
            source,
            target,
            fs_type,
            is_network,
            label,
        });
    }

    // Root first, then alphabetical.
    out.sort_by(|a, b| {
        let root = Path::new("/");
        let a_root = a.target == root;
        let b_root = b.target == root;
        b_root
            .cmp(&a_root)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });

    out
}

/// `/proc/mounts` escapes spaces/tabs/backslashes as `\040`, `\011`,
/// `\012`, `\134`, etc. Unescape them so paths display correctly.
///
/// Escape-sequence scanning operates on raw bytes (since `\040` is
/// ASCII and byte-aligned). The unescaped output is collected as raw
/// `u8` bytes and decoded via `String::from_utf8_lossy` once at the
/// end. This preserves multi-byte UTF-8 sequences in mount point names
/// (accented letters, non-Latin scripts) — per-byte `as char` casts
/// would corrupt them into separate garbage codepoints.
fn unescape_octal(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());

    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() {
            if let (Some(a), Some(b), Some(c)) = (
                (bytes[i + 1] as char).to_digit(8),
                (bytes[i + 2] as char).to_digit(8),
                (bytes[i + 3] as char).to_digit(8),
            ) {
                let code = a * 64 + b * 8 + c;
                // Push the raw byte. For ASCII escapes (space, tab, etc.)
                // this is the literal byte; for high-bit codes the
                // `from_utf8_lossy` call below reconstructs the proper
                // UTF-8 sequence from the surrounding bytes.
                out.push(code as u8);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sample_proc_mounts() {
        let sample = "\
rootfs / rootfs rw 0 0
/dev/sda2 / ext4 rw,relatime 0 0
proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
tmpfs /tmp tmpfs rw,nosuid,nodev 0 0
/dev/sda1 /boot ext4 rw,relatime 0 0
/dev/sdb1 /mnt/usb vfat rw,relatime 0 0
server:/export /mnt/nfs nfs4 rw,relatime 0 0
";
        let mounts = parse_mounts(sample);

        // `rootfs / rootfs` is the kernel's initial rootfs mount. It is
        // not in PSEUDO_FS; some systems expose it as a real mount. It
        // mounts on `/`, same as the ext4 entry, so the dedup logic
        // keeps whichever appears first (rootfs wins). Then /boot,
        // /mnt/usb, /mnt/nfs alphabetically by label. Pseudo-fses
        // (proc, sysfs, tmpfs) are filtered out.
        assert_eq!(mounts.len(), 4);
        assert_eq!(mounts[0].target, PathBuf::from("/"));
        // First `/` entry wins: rootfs
        assert_eq!(mounts[0].fs_type, "rootfs");
        assert!(!mounts[0].is_network);

        // boot, nfs, usb alphabetically by label (lowercased)
        assert_eq!(mounts[1].label, "boot");
        assert_eq!(mounts[2].label, "nfs");
        assert_eq!(mounts[3].label, "usb");
        assert!(mounts[2].is_network);
        assert_eq!(mounts[2].fs_type, "nfs4");
        assert_eq!(mounts[2].source, "server:/export");
    }

    #[test]
    fn parse_filters_pseudo_filesystems() {
        let sample = "\
/dev/sda1 / ext4 rw 0 0
proc /proc proc rw 0 0
sysfs /sys sysfs rw 0 0
tmpfs /tmp tmpfs rw 0 0
cgroup2 /sys/fs/cgroup cgroup2 rw 0 0
fuse.gvfsd-fuse /run/user/1000/gvfs fuse.gvfsd-fuse rw 0 0
";
        let mounts = parse_mounts(sample);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].target, PathBuf::from("/"));
    }

    #[test]
    fn parse_tags_network_filesystems() {
        let sample = "\
/dev/sda1 / ext4 rw 0 0
server:/share /mnt/nfs nfs rw 0 0
//server/share /mnt/smb cifs rw 0 0
user@host:/path /mnt/ssh sshfs rw 0 0
";
        let mounts = parse_mounts(sample);
        assert_eq!(mounts.len(), 4);
        let network_count = mounts.iter().filter(|m| m.is_network).count();
        assert_eq!(network_count, 3);
    }

    #[test]
    fn parse_dedupes_duplicate_targets() {
        // overlay-fs on top of / is a common pattern
        let sample = "\
/dev/sda1 / ext4 rw 0 0
overlay / ext4 rw 0 0
/dev/sda1 /boot ext4 rw 0 0
";
        let mounts = parse_mounts(sample);
        // First "/" wins; second "/" is skipped; "/boot" remains.
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].target, PathBuf::from("/"));
        assert_eq!(mounts[1].target, PathBuf::from("/boot"));
    }

    #[test]
    fn parse_handles_empty_input() {
        let mounts = parse_mounts("");
        assert!(mounts.is_empty());
    }

    #[test]
    fn parse_skips_malformed_lines() {
        let sample = "\
/dev/sda1 / ext4 rw 0 0
incomplete line
also incomplete
/dev/sda1 /boot ext4 rw 0 0
";
        let mounts = parse_mounts(sample);
        assert_eq!(mounts.len(), 2);
    }

    #[test]
    fn unescape_spaces() {
        assert_eq!(unescape_octal("/mnt/My\\040Drive"), "/mnt/My Drive");
        assert_eq!(unescape_octal("/normal/path"), "/normal/path");
    }

    #[test]
    fn handles_octal_escapes() {
        assert_eq!(unescape_octal("a\\011b"), "a\tb");
        assert_eq!(unescape_octal("a\\134b"), "a\\b");
    }

    #[test]
    fn unescape_preserves_utf8() {
        // Mount points can contain non-ASCII characters (accented letters,
        // non-Latin scripts). The escape sequence scanner walks raw bytes,
        // but multi-byte UTF-8 sequences must pass through unchanged.
        let input = "/mnt/café";
        let output = unescape_octal(input);
        assert_eq!(output, "/mnt/café");
        assert_eq!(output.chars().count(), 9); // 9 codepoints, not 10 bytes

        // Cyrillic: 2-byte UTF-8 sequences
        let input = "/mnt/Документы";
        let output = unescape_octal(input);
        assert_eq!(output, "/mnt/Документы");

        // Mixed: escape sequence + UTF-8
        let input = "/mnt/My\\040Документы";
        let output = unescape_octal(input);
        assert_eq!(output, "/mnt/My Документы");
    }
}
