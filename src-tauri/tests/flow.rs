//! End-to-end flows through `Core` against a fake game folder in a temp directory.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use kobold_lib::service::{Core, Progress};

fn noop(_: Progress) {}

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    core: Core,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let wow = tmp.path().join("World of Warcraft");
    let root = wow.join("_forever_");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("Wow.exe"), b"").unwrap();
    fs::write(
        wow.join(".build.info"),
        "Branch!STRING:0|Version!STRING:0|Product!STRING:0\nus|1.60.1.70790|wowf\n",
    )
    .unwrap();
    let base = tmp.path().join("app");
    let core = Core::new(
        base.join("data"),
        base.join("cache"),
        base.join("logs"),
        None,
    );
    core.set_game_root(&wow).unwrap();
    Fixture {
        _tmp: tmp,
        root,
        core,
    }
}

fn zip(path: &Path, files: &[(&str, &str)]) {
    let mut z = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, body) in files {
        z.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(body.as_bytes()).unwrap();
    }
    z.finish().unwrap();
}

#[tokio::test]
async fn import_toggle_remove_undo() {
    let f = fixture();
    let addons = f.root.join("Interface").join("AddOns");
    let archive = f._tmp.path().join("Bagnon.zip");
    zip(
        &archive,
        &[
            (
                "Bagnon/Bagnon_Forever.toc",
                "## Interface: 16001\n## Title: Bagnon\n## Version: 1.60.3\n## Author: Jaliborc\n",
            ),
            ("Bagnon/Bagnon.lua", "-- code"),
            ("Bagnon_Config/Bagnon_Config.toc", "## Interface: 16001\n"),
        ],
    );

    // Install from zip: folder is created, both addon folders land, metadata comes from the TOC.
    let v = f.core.import_zip(&archive, &noop).await.unwrap();
    assert_eq!(v.name, "Bagnon");
    assert_eq!(v.version.as_deref(), Some("1.60.3"));
    assert!(v.built_for_forever);
    assert_eq!(v.folders, vec!["Bagnon", "Bagnon_Config"]);
    assert!(addons.join("Bagnon/Bagnon.lua").exists());
    assert!(
        !f.root.join("Interface/.kobold-staging").exists(),
        "staging is cleaned up"
    );

    // Re-import the same zip (an update) keeps a single entry.
    f.core.import_zip(&archive, &noop).await.unwrap();
    assert_eq!(f.core.list_installed().unwrap().len(), 1);

    // Disable hides the folders from the game, enable brings them back.
    f.core.set_enabled(&v.id, false).await.unwrap();
    assert!(!addons.join("Bagnon").exists());
    assert!(!f.core.list_installed().unwrap()[0].enabled);
    f.core.set_enabled(&v.id, true).await.unwrap();
    assert!(addons.join("Bagnon").exists());

    // Delete previews exactly what goes, then removes it.
    let p = f.core.delete_preview(&v.id).unwrap();
    assert_eq!(p.folders.len(), 2);
    let removed = f.core.remove(&v.id, false).await.unwrap();
    assert!(!addons.join("Bagnon").exists());
    assert!(f.core.list_installed().unwrap().is_empty());

    // Undo restores both the folders and the manifest entry. Some machines (CI
    // runners, drives with the Recycle Bin turned off) delete permanently; then undo
    // must say so and leave the list alone rather than point at missing folders.
    match f.core.undo_remove(&removed.token).await {
        Ok(()) => {
            assert!(addons.join("Bagnon/Bagnon.lua").exists());
            assert_eq!(f.core.list_installed().unwrap().len(), 1);
            f.core.remove(&v.id, false).await.unwrap();
        }
        Err(e) => {
            assert_eq!(e.code(), "not_found", "unexpected undo error: {e}");
            assert!(f.core.list_installed().unwrap().is_empty());
            eprintln!("no usable Recycle Bin here; checked the fallback instead");
        }
    }
}

#[tokio::test]
async fn hand_installed_addons_are_listed_and_grouped() {
    let f = fixture();
    let addons = f.root.join("Interface").join("AddOns");
    for (dir, toc) in [
        ("MyBars", "## Title: My Bars\n## Interface: 120100\n"),
        ("MyBars_Options", "## Interface: 16001\n"),
    ] {
        fs::create_dir_all(addons.join(dir)).unwrap();
        fs::write(addons.join(dir).join(format!("{dir}.toc")), toc).unwrap();
    }
    fs::create_dir_all(addons.join("NotAnAddon")).unwrap();

    let list = f.core.list_installed().unwrap();
    assert_eq!(list.len(), 1);
    let a = &list[0];
    assert_eq!(a.id, "local:MyBars");
    assert_eq!(a.name, "My Bars");
    assert!(a.source.is_none());
    assert!(!a.built_for_forever);
    assert_eq!(a.folders, vec!["MyBars", "MyBars_Options"]);
}

#[tokio::test]
async fn bad_archive_changes_nothing() {
    let f = fixture();
    let addons = f.root.join("Interface").join("AddOns");
    fs::create_dir_all(addons.join("Keep")).unwrap();
    fs::write(addons.join("Keep/Keep.toc"), "").unwrap();

    let junk = f._tmp.path().join("junk.zip");
    fs::write(&junk, b"this is not a zip").unwrap();
    assert!(f.core.import_zip(&junk, &noop).await.is_err());

    let empty = f._tmp.path().join("empty.zip");
    zip(&empty, &[("readme.txt", "hi")]);
    let err = f.core.import_zip(&empty, &noop).await.unwrap_err();
    assert_eq!(err.code(), "no_addon");

    assert!(addons.join("Keep/Keep.toc").exists());
    assert_eq!(f.core.list_installed().unwrap().len(), 1);
}

#[tokio::test]
async fn manifest_survives_restart() {
    let f = fixture();
    let archive = f._tmp.path().join("A.zip");
    zip(&archive, &[("A/A.toc", "## Interface: 16001\n")]);
    f.core.import_zip(&archive, &noop).await.unwrap();
    let base = f._tmp.path().join("app");
    let again = Core::new(
        base.join("data"),
        base.join("cache"),
        base.join("logs"),
        None,
    );
    let list = again.list_installed().unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].source.is_some());
}

#[tokio::test]
#[ignore]
async fn live_github_resolve() {
    let http = kobold_lib::net::client();
    let gh = kobold_lib::sources::github::GitHub { http: &http };
    for repo in [
        "BigWigsMods/BigWigs",
        "Tercioo/Details-Damage-Meter",
        "Jaliborc/Bagnon",
    ] {
        let (o, r) = repo.split_once('/').unwrap();
        match gh.resolve(o, r).await {
            Ok(res) => println!(
                "{repo}: {} -> {} verified={:?}",
                res.version,
                res.download_url,
                !matches!(res.expected, kobold_lib::net::Expected::None)
            ),
            Err(e) => println!("{repo}: ERR {e}"),
        }
    }
}

/// Copies this PC's saved CurseForge key into a fresh data folder. DPAPI keys only
/// decrypt for the same Windows user, so this only works on the machine that saved it.
fn live_fixture(manifest: Option<&str>) -> Option<Fixture> {
    let key = PathBuf::from(std::env::var("APPDATA").ok()?)
        .join("com.rique.koboldclient")
        .join("curseforge.key.bin");
    if !key.exists() {
        println!("no CurseForge key on this PC; skipping");
        return None;
    }
    let tmp = tempfile::tempdir().unwrap();
    let wow = tmp.path().join("World of Warcraft");
    let root = wow.join("_forever_");
    fs::create_dir_all(root.join("Interface").join("AddOns")).unwrap();
    fs::write(root.join("Wow.exe"), b"").unwrap();
    fs::write(
        wow.join(".build.info"),
        "Branch!STRING:0|Version!STRING:0|Product!STRING:0\nus|1.60.1.70790|wowf\n",
    )
    .unwrap();
    let data = tmp.path().join("app").join("data");
    fs::create_dir_all(&data).unwrap();
    fs::copy(&key, data.join("curseforge.key.bin")).unwrap();
    if let Some(m) = manifest {
        fs::write(data.join("manifest.json"), m).unwrap();
    }
    let base = tmp.path().join("app");
    let core = Core::new(data, base.join("cache"), base.join("logs"), None);
    core.set_game_root(&wow).unwrap();
    Some(Fixture {
        _tmp: tmp,
        root,
        core,
    })
}

#[tokio::test]
#[ignore]
async fn live_curseforge_requirements() {
    // A Questie entry as 0.1.1 wrote it: no requirement list, QuestieDB not installed.
    let legacy = r#"{"schema":1,"addons":[{"id":"cf:334372","name":"Questie","source":"curseforge","version":"v12.6.0","file_id":9444002,"folders":["Questie"]}]}"#;
    let Some(f) = live_fixture(Some(legacy)) else {
        return;
    };
    let r = f.core.repair_requirements(&noop).await.unwrap();
    println!("repair: {r:?}");
    assert!(r.failed.is_empty());
    for name in ["QuestieDB", "QuestieTrace"] {
        assert!(
            r.installed.iter().any(|n| n == name),
            "{name} not installed"
        );
    }
    let addons = f.root.join("Interface").join("AddOns");
    for e in fs::read_dir(&addons).unwrap().flatten() {
        println!("  AddOns/{}", e.file_name().to_string_lossy());
    }
    let list = f.core.list_installed().unwrap();
    let questie = list.iter().find(|a| a.id == "cf:334372").unwrap();
    assert!(questie.missing_deps.is_empty());
    // A second run has nothing left to do and makes no requests.
    let again = f.core.repair_requirements(&noop).await.unwrap();
    assert!(again.installed.is_empty());

    // A fresh install of Questie pulls both in.
    let Some(g) = live_fixture(None) else { return };
    let out = g.core.install("cf:334372", &noop).await.unwrap();
    println!("install: {} also {:?}", out.addon.name, out.also_installed);
    assert_eq!(out.also_installed.len(), 2);
    assert!(out.addon.missing_deps.is_empty());
}
