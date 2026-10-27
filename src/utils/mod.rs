//! Small helpers shared by several modules.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

type NameTable = (Option<SystemTime>, HashMap<u32, String>);

/// id -> name from an /etc/passwd-style file, re-read when the file changes.
fn lookup_name(file: &'static str, cache: &'static OnceLock<Mutex<NameTable>>, id: u32) -> String {
    let stamp = std::fs::metadata(file).and_then(|m| m.modified()).ok();
    let table = cache.get_or_init(|| Mutex::new((None, HashMap::new())));
    let Ok(mut table) = table.lock() else {
        return id.to_string();
    };
    if table.0 != stamp || table.1.is_empty() {
        let names = std::fs::read_to_string(file)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let mut cols = line.split(':');
                let name = cols.next()?;
                let id: u32 = cols.nth(1)?.parse().ok()?;
                Some((id, name.to_string()))
            })
            .collect();
        *table = (stamp, names);
    }
    table.1.get(&id).cloned().unwrap_or_else(|| id.to_string())
}

/// All entries of an /etc/passwd-style file, sorted by id.
fn all_names(file: &str) -> Vec<(u32, String)> {
    let mut names: Vec<(u32, String)> = std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut cols = line.split(':');
            let name = cols.next()?.to_string();
            let id: u32 = cols.nth(1)?.parse().ok()?;
            Some((id, name))
        })
        .collect();
    names.sort();
    names
}

pub fn all_users() -> Vec<(u32, String)> {
    all_names("/etc/passwd")
}

pub fn all_groups() -> Vec<(u32, String)> {
    all_names("/etc/group")
}

/// A user given by name or number.
pub fn uid_of(user: &str) -> Option<u32> {
    let user = user.trim();
    user.parse().ok().or_else(|| {
        all_users()
            .into_iter()
            .find(|(_, n)| n == user)
            .map(|(id, _)| id)
    })
}

/// A group given by name or number.
pub fn gid_of(group: &str) -> Option<u32> {
    let group = group.trim();
    group.parse().ok().or_else(|| {
        all_groups()
            .into_iter()
            .find(|(_, n)| n == group)
            .map(|(id, _)| id)
    })
}

/// The user name of a uid (the number itself when unknown).
pub fn user_name(uid: u32) -> String {
    static USERS: OnceLock<Mutex<NameTable>> = OnceLock::new();
    lookup_name("/etc/passwd", &USERS, uid)
}

/// The group name of a gid (the number itself when unknown).
pub fn group_name(gid: u32) -> String {
    static GROUPS: OnceLock<Mutex<NameTable>> = OnceLock::new();
    lookup_name("/etc/group", &GROUPS, gid)
}

#[cfg(test)]
mod tests {
    #[test]
    fn root_has_a_name() {
        assert_eq!(super::user_name(0), "root");
        assert_eq!(super::group_name(0), "root");
        assert_eq!(super::user_name(4_000_000_000), "4000000000");
        assert_eq!(super::uid_of("root"), Some(0));
        assert_eq!(super::uid_of("1234"), Some(1234));
        assert_eq!(super::gid_of("no-such-group-xyz"), None);
    }
}
