use assert_cmd::prelude::*;
use std::process::Command;

#[test]
fn runs_collect_command() {
    let mut cmd = Command::cargo_bin("veri-cli").unwrap();
    cmd.arg("--help");
    cmd.assert().success();
}
