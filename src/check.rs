//! `check`: every finding for a loaded store.

use std::collections::{BTreeMap, BTreeSet};

use crate::finding::{self, Code, Finding};
use crate::index;
use crate::note::Note;
use crate::store::{INDEX_FILE, IndexFile, Store};

/// Every finding for `store`, sorted as [`finding::sort`] sorts them.
#[must_use]
pub fn check(store: &Store) -> Vec<Finding> {
    let mut findings = store.findings.clone();
    let mut by_name: BTreeMap<&str, Vec<&Note>> = BTreeMap::new();
    for note in &store.notes {
        findings.extend(note.findings.iter().cloned());
        if let Some((name, _)) = &note.name {
            by_name.entry(name.as_str()).or_default().push(note);
        }
    }
    duplicate_names(&by_name, &mut findings);
    dangling_links(store, &by_name, &mut findings);
    index_findings(store, &mut findings);
    finding::sort(&mut findings);
    findings
}

fn duplicate_names(by_name: &BTreeMap<&str, Vec<&Note>>, findings: &mut Vec<Finding>) {
    for (name, notes) in by_name.iter().filter(|(_, notes)| notes.len() > 1) {
        for note in notes {
            let others: Vec<&str> = notes
                .iter()
                .filter(|other| other.path != note.path)
                .map(|other| other.path.as_str())
                .collect();
            findings.push(Finding::new(
                note.path.as_str(),
                note.name.as_ref().map(|(_, line)| *line),
                Code::DuplicateName,
                format!("`{name}` is also the name of {}", others.join(", ")),
            ));
        }
    }
}

fn dangling_links(
    store: &Store,
    by_name: &BTreeMap<&str, Vec<&Note>>,
    findings: &mut Vec<Finding>,
) {
    for note in &store.notes {
        for link in &note.links {
            if !by_name.contains_key(link.target.as_str()) {
                findings.push(Finding::new(
                    note.path.as_str(),
                    Some(link.line),
                    Code::DanglingLink,
                    format!("no note is named `{}`", link.target),
                ));
            }
        }
    }
}

fn index_findings(store: &Store, findings: &mut Vec<Finding>) {
    let text = match &store.index {
        IndexFile::Missing => {
            findings.push(Finding::new(
                INDEX_FILE,
                None,
                Code::IndexMissing,
                format!("the store root has no {INDEX_FILE}"),
            ));
            return;
        }
        IndexFile::NotUtf8 => {
            findings.push(Finding::new(
                INDEX_FILE,
                None,
                Code::Encoding,
                "index is not valid UTF-8",
            ));
            return;
        }
        IndexFile::Text(text) => text,
    };

    let paths: BTreeSet<&str> = store.notes.iter().map(|note| note.path.as_str()).collect();
    let entries = index::entries(text);
    let mut listed: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in &entries {
        let target = entry.target.as_str();
        if !paths.contains(target) {
            findings.push(Finding::new(
                INDEX_FILE,
                Some(entry.line),
                Code::StaleIndexEntry,
                format!("`{target}` is not a note in the store"),
            ));
        } else if let Some(first) = listed.get(target) {
            findings.push(Finding::new(
                INDEX_FILE,
                Some(entry.line),
                Code::DuplicateIndexEntry,
                format!("`{target}` is already listed on line {first}"),
            ));
        } else {
            listed.insert(target, entry.line);
        }
    }
    for note in &store.notes {
        if !listed.contains_key(note.path.as_str()) {
            findings.push(Finding::new(
                note.path.as_str(),
                None,
                Code::Orphan,
                format!("note is not listed in {INDEX_FILE}"),
            ));
        }
    }
}
