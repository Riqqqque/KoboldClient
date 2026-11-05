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
    let gh = kobold_lib::sources::github::GitHub {
        http: &http,
        token: None,
    };
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

/// Read-only: fingerprints this PC's CurseForge addons and checks them against what
/// CurseForge lists for the installed files.
/// `cargo test --test flow live_fingerprints -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn live_fingerprints() {
    let data = PathBuf::from(std::env::var("APPDATA").unwrap()).join("com.rique.koboldclient");
    let Some(key) = kobold_lib::secret::load_cf_key(&data) else {
        println!("no CurseForge key on this PC; skipping");
        return;
    };
    let settings = kobold_lib::settings::load(&data);
    let game = kobold_lib::game::validate(&settings.game_root.unwrap()).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(data.join("manifest.json")).unwrap()).unwrap();
    let file_ids: Vec<u64> = manifest["addons"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["file_id"].as_u64())
        .collect();
    let http = kobold_lib::net::client();
    let cell = tokio::sync::OnceCell::new();
    let cf = kobold_lib::sources::curseforge::CurseForge {
        http: &http,
        key: &key,
        forever_type: &cell,
    };
    let modules = cf.file_modules(&file_ids).await.unwrap();
    let (mut same, mut total) = (0, 0);
    let mut local = vec![];
    for (file, mods) in &modules {
        for m in mods {
            let fp = kobold_lib::fingerprint::folder_fingerprint(&game.addons_dir.join(&m.name));
            total += 1;
            if fp.map(u64::from) == Some(m.fingerprint) {
                same += 1;
            }
            if let Some(f) = fp {
                local.push(f);
            }
            println!(
                "file {file} {:<20} curseforge={} local={:?}",
                m.name, m.fingerprint, fp
            );
        }
    }
    let matches = cf.match_fingerprints(&local).await.unwrap();
    for m in &matches {
        println!(
            "match mod {} file {} {}",
            m.mod_id, m.file_id, m.display_name
        );
    }
    println!("{same}/{total} folders match");
    assert_eq!(same, total);
}

fn write_sv(root: &Path, addon: &str, body: &str) -> PathBuf {
    let dir = root.join("WTF/Account/ACC1/SavedVariables");
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{addon}.lua"));
    fs::write(&p, body).unwrap();
    p
}

#[tokio::test]
async fn updates_back_up_settings_and_restore_works() {
    let f = fixture();
    let sv = write_sv(&f.root, "Bagnon", "BagnonDB = { v = 1 }");
    let archive = f._tmp.path().join("Bagnon.zip");
    zip(
        &archive,
        &[("Bagnon/Bagnon.toc", "## Interface: 16001\n## Version: 1\n")],
    );

    // A first install isn't an update, so no backup.
    f.core.import_zip(&archive, &noop).await.unwrap();
    assert!(f.core.list_backups().is_empty());

    // An update backs up once; another update with no settings change doesn't.
    f.core.import_zip(&archive, &noop).await.unwrap();
    assert_eq!(f.core.list_backups().len(), 1);
    f.core.import_zip(&archive, &noop).await.unwrap();
    assert_eq!(f.core.list_backups().len(), 1);

    // The setting turns it off.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(&sv, "BagnonDB = { v = 2 }").unwrap();
    f.core
        .update_settings(|s| s.backup_before_update = false)
        .unwrap();
    f.core.import_zip(&archive, &noop).await.unwrap();
    assert_eq!(f.core.list_backups().len(), 1);
    f.core
        .update_settings(|s| s.backup_before_update = true)
        .unwrap();

    // Restore brings v1 back and keeps the replaced settings as a backup.
    let first = f.core.list_backups()[0].name.clone();
    f.core.restore_backup(&first).await.unwrap();
    assert_eq!(fs::read_to_string(&sv).unwrap(), "BagnonDB = { v = 1 }");
    let all = f.core.list_backups();
    assert_eq!(all.len(), 2);
    assert!(all
        .iter()
        .any(|b| b.kind == kobold_lib::backup::Kind::Restore));

    let manual = f.core.backup_now().await.unwrap();
    f.core.delete_backup(&manual.name).unwrap();
    assert!(f.core.delete_backup("../manifest.json").is_err());
    assert!(f.core.restore_backup("wtf-1-auto.zip").await.is_err());
}

#[tokio::test]
async fn lua_errors_are_grouped_by_addon() {
    let f = fixture();
    let archive = f._tmp.path().join("Bagnon.zip");
    zip(
        &archive,
        &[(
            "Bagnon/Bagnon.toc",
            "## Interface: 16001\n## Title: Bagnon\n",
        )],
    );
    f.core.import_zip(&archive, &noop).await.unwrap();
    let hand = f.root.join("Interface/AddOns/TomTom");
    fs::create_dir_all(&hand).unwrap();
    fs::write(hand.join("TomTom.toc"), "## Interface: 16001\n").unwrap();

    assert_eq!(f.core.lua_errors().unwrap().total, 0, "no log yet");
    let logs = f.root.join("Logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(
        logs.join("General.log"),
        "11/4 20:00:00.000  [N][Console] start\n\
11/4 20:01:00.000  [E][Lua] Lua Error: Interface/AddOns/Bagnon/Core.lua:5: attempt to call a nil value\n\
[Interface/AddOns/Bagnon/Core.lua]:5: in main chunk\n\
\n\
11/4 20:02:00.000  [E][Lua] Lua Error: Interface/AddOns/Bagnon/Core.lua:5: attempt to call a nil value\n\
11/4 20:03:00.000  [E][Lua] Lua Error: ...ddOns/Blizzard_UnitFrame/X.lua:1: secret (execution tainted by 'TomTom')\n\
11/4 20:04:00.000  [E][Lua] Lua Error: [string \"/run x()\"]:1: attempt to call a nil value\n",
    )
    .unwrap();
    let r = f.core.lua_errors().unwrap();
    assert_eq!(r.total, 3);
    assert_eq!(r.started.as_deref(), Some("11/4 20:00:00"));
    assert_eq!(r.addons[0].name, "Bagnon");
    assert_eq!(r.addons[0].id.as_deref(), Some("zip:bagnon"));
    assert_eq!(r.addons[0].groups.len(), 1);
    assert_eq!(r.addons[0].groups[0].count, 2);
    assert_eq!(r.addons[0].groups[0].last, "11/4 20:02:00");
    assert_eq!(r.addons[1].id.as_deref(), Some("local:TomTom"));
    assert_eq!(r.other.len(), 1);
    assert!(!r.seen);
    f.core.dismiss_lua_errors(&r.session).unwrap();
    assert!(f.core.lua_errors().unwrap().seen);
}

#[tokio::test]
async fn addon_list_round_trip() {
    let f = fixture();
    let archive = f._tmp.path().join("Bagnon.zip");
    zip(
        &archive,
        &[(
            "Bagnon/Bagnon.toc",
            "## Interface: 16001\n## Title: Bagnon\n",
        )],
    );
    f.core.import_zip(&archive, &noop).await.unwrap();
    let out = f._tmp.path().join("my-addons.json");
    assert_eq!(f.core.export_addon_list(&out).unwrap(), 1);
    let same = f.core.preview_addon_list(&out).unwrap();
    assert_eq!(same.present.len(), 1);

    // On another PC: the zip addon has to be installed by hand, the rest by Kobold.
    let g = fixture();
    let text = fs::read_to_string(&out).unwrap().replace(
        "\"addons\": [",
        "\"addons\": [{\"id\":\"cf:334372\",\"name\":\"Questie\",\"pinned\":true},{\"id\":\"wowi:27236\",\"name\":\"VuloForeverUI\"},",
    );
    fs::write(&out, text).unwrap();
    let p = g.core.preview_addon_list(&out).unwrap();
    assert_eq!(p.to_install.len(), 2);
    assert!(p.to_install[0].pinned);
    assert_eq!(p.manual.len(), 1);
    assert_eq!(p.manual[0].name, "Bagnon");
}

/// `cargo test --test flow live_wowinterface -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn live_wowinterface() {
    let f = fixture();
    let found = f
        .core
        .search("wowinterface", "forever", "popular")
        .await
        .unwrap();
    println!(
        "search: {} results, first {:?}",
        found.len(),
        found.first().map(|r| &r.name)
    );
    assert!(!found.is_empty());
    let one = f
        .core
        .search(
            "wowinterface",
            "https://www.wowinterface.com/downloads/info27236-VuloForeverUI.html",
            "",
        )
        .await
        .unwrap();
    assert_eq!(one[0].id, "wowi:27236");
    // Downloads and checks the MD5 WoWInterface publishes.
    let out = f.core.install("wowi:27236", &noop).await.unwrap();
    println!(
        "installed {} {} forever={}",
        out.addon.name,
        out.addon.version.as_deref().unwrap_or("?"),
        out.addon.built_for_forever
    );
    assert!(f
        .root
        .join("Interface/AddOns")
        .join(&out.addon.folders[0])
        .exists());
    let up = f.core.check_updates().await.unwrap();
    assert!(up.is_empty(), "just installed, so no update");
}

/// Without a key every Wago call must fail cleanly, never reach the network unasked.
#[tokio::test]
async fn wago_needs_a_key() {
    let f = fixture();
    let e = f.core.search("wago", "bigwigs", "").await.unwrap_err();
    assert_eq!(e.code(), "no_wago_key");
    let e = f.core.install("wago:5NRegwG3", &noop).await.unwrap_err();
    assert_eq!(e.code(), "no_wago_key");
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

/// Copies two of this PC's addon folders into a temp game folder as if installed by
/// hand, then links them. The real game folder is only read.
/// `cargo test --test flow live_link_hand_installed -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn live_link_hand_installed() {
    let Some(f) = live_fixture(None) else { return };
    let data = PathBuf::from(std::env::var("APPDATA").unwrap()).join("com.rique.koboldclient");
    let real =
        kobold_lib::game::validate(&kobold_lib::settings::load(&data).game_root.unwrap()).unwrap();
    let addons = f.root.join("Interface/AddOns");
    for name in ["TomTom", "Pawn"] {
        copy_dir(&real.addons_dir.join(name), &addons.join(name));
    }
    // An unrelated folder that CurseForge won't know.
    fs::create_dir_all(addons.join("MyOwnThing")).unwrap();
    fs::write(
        addons.join("MyOwnThing/MyOwnThing.toc"),
        "## Interface: 16001\nMain.lua\n",
    )
    .unwrap();
    fs::write(
        addons.join("MyOwnThing/Main.lua"),
        "print('mine ' .. 123456789)",
    )
    .unwrap();

    let before = f.core.list_installed().unwrap();
    assert!(before.iter().all(|a| a.source.is_none()));
    let out = f.core.link_hand_installed().await.unwrap();
    println!("linked: {:?}", out.linked);
    let after = f.core.list_installed().unwrap();
    for a in &after {
        println!("  {} {:?} {:?} {:?}", a.id, a.source, a.version, a.folders);
    }
    assert!(after.iter().any(|a| a.id == "cf:18808"), "TomTom linked");
    assert!(after.iter().any(|a| a.id == "cf:4646"), "Pawn linked");
    assert!(after.iter().any(|a| a.id == "local:MyOwnThing"));
    // Running it again finds nothing new.
    assert!(f
        .core
        .link_hand_installed()
        .await
        .unwrap()
        .linked
        .is_empty());
    let up = f.core.check_updates().await.unwrap();
    println!("updates after linking: {}", up.len());
}
