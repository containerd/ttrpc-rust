use std::{
    io::{BufRead, BufReader},
    process::Command,
    time::Duration,
};

#[test]
fn run_sync_example() -> Result<(), Box<dyn std::error::Error>> {
    // start the server and give it a moment to start.
    let mut server = run_example("server").spawn().unwrap();
    std::thread::sleep(Duration::from_secs(2));

    let mut client = run_example("client").spawn().unwrap();
    let mut client_succeeded = false;
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(600);
    loop {
        if start.elapsed() > timeout {
            println!("Running the client timed out. output:");
            client.kill().unwrap_or_else(|e| {
                println!("This may occur on Windows if the process has exited: {e}");
            });
            let output = client.stdout.unwrap();
            BufReader::new(output).lines().for_each(|line| {
                println!("{}", line.unwrap());
            });
            break;
        }

        match client.try_wait() {
            Ok(Some(status)) => {
                client_succeeded = status.success();
                break;
            }
            Ok(None) => {
                // still running
                continue;
            }
            Err(e) => {
                println!("Error: {e}");
                break;
            }
        }
    }

    // be sure to clean up the server, the client should have run to completion
    server.kill()?;
    assert!(client_succeeded);
    Ok(())
}

fn run_example(example: &str) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .arg("--example")
        .arg(example)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir("example");
    cmd
}
