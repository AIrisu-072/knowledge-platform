//! Native redb P3-P04 process test. Run only in the released Cargo window.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

fn request(stdin: &mut impl Write, stdout: &mut impl BufRead, id: u64, command: Value) -> Value {
    let mut value = command;
    value["id"] = json!(id);
    writeln!(stdin, "{value}").unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["id"], id);
    assert!(response.get("error").is_none(), "{response}");
    response["ok"].clone()
}

#[test]
fn direct_readers_writer_restart_and_offline_copy() {
    let binary = env!("CARGO_BIN_EXE_qualification_redb");
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/refinement-fixture.json");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("p3-qual-redb-{}-{suffix}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let db = directory.join("primary.redb");
    let stage = Command::new(binary)
        .args(["stage", fixture, db.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        stage.status.success(),
        "{}",
        String::from_utf8_lossy(&stage.stderr)
    );
    let receipt: Value = serde_json::from_slice(&stage.stdout).unwrap();
    for phase in [
        "baseline_full_ns",
        "updated_incremental_ns",
        "updated_full_ns",
    ] {
        assert!(receipt[phase].as_u64().unwrap() > 0, "{receipt}");
    }
    let mut child = Command::new(binary)
        .args(["serve", db.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let source = "00000000-0000-0000-0000-000000000001";
    let generation = "00000000-0000-0000-0000-000000000064";
    let seed = "00000000-0000-0000-0000-000000002710";
    let first = request(
        &mut stdin,
        &mut stdout,
        1,
        json!({"cmd":"frontier",
        "source":source,"generation":generation,"resources":[seed],"role":"borrower"}),
    );
    assert!(first.as_array().unwrap().len() > 1);
    let authority = request(
        &mut stdin,
        &mut stdout,
        6,
        json!({"cmd":"authority",
        "source":source,"scenario":"source-one-revoked","resource":seed}),
    );
    assert_eq!(authority[0], "2");
    request(&mut stdin, &mut stdout, 2, json!({"cmd":"writer_start"}));
    std::thread::sleep(Duration::from_millis(50));
    let second = request(
        &mut stdin,
        &mut stdout,
        3,
        json!({"cmd":"frontier",
        "source":source,"generation":generation,"resources":[seed],"role":"borrower"}),
    );
    assert_eq!(first, second);
    let updates = request(&mut stdin, &mut stdout, 4, json!({"cmd":"writer_stop"}));
    assert!(!updates.as_array().unwrap().is_empty());
    drop(stdin);
    assert!(child.wait().unwrap().success());
    let backup = directory.join("backup.redb");
    fs::copy(&db, &backup).unwrap();
    let mut restored = Command::new(binary)
        .args(["serve", backup.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut restored_in = restored.stdin.take().unwrap();
    let mut restored_out = BufReader::new(restored.stdout.take().unwrap());
    let from_backup = request(
        &mut restored_in,
        &mut restored_out,
        5,
        json!({"cmd":"frontier",
        "source":source,"generation":generation,"resources":[seed],"role":"borrower"}),
    );
    assert_eq!(first, from_backup);
    drop(restored_in);
    assert!(restored.wait().unwrap().success());
    let mut corrupt = Command::new(binary)
        .args(["serve", backup.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut corrupt_in = corrupt.stdin.take().unwrap();
    let mut corrupt_out = BufReader::new(corrupt.stdout.take().unwrap());
    request(
        &mut corrupt_in,
        &mut corrupt_out,
        7,
        json!({"cmd":"corrupt_reverse_incidence",
        "source":source,"generation":generation,
        "relation":"00000000-0000-0000-0000-0000000f4240","ordinal":0}),
    );
    writeln!(
        corrupt_in,
        "{}",
        json!({"id":8,"cmd":"frontier", "source":source,
        "generation":generation,"resources":[seed],"role":"borrower"})
    )
    .unwrap();
    corrupt_in.flush().unwrap();
    let mut line = String::new();
    corrupt_out.read_line(&mut line).unwrap();
    let failure: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(failure["id"], 8);
    assert!(
        failure["error"]
            .as_str()
            .unwrap()
            .contains("reverse incidence")
    );
    drop(corrupt_in);
    assert!(corrupt.wait().unwrap().success());
    fs::remove_dir_all(directory).unwrap();
}
