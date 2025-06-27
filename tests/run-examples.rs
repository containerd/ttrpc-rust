use std::{
    io::{BufRead, BufReader},
    process::{Child, Command},
    time::Duration,
};

fn run_example(
    server: &str,
    client: &str,
    args: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    // start the server and give it a moment to start.
    let mut server = do_run_example(server, args).spawn().unwrap();
    std::thread::sleep(Duration::from_secs(2));

    let mut client = do_run_example(client, args).spawn().unwrap();
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
                println!("client output: {}", line.unwrap());
            });
            break;
        }

        match client.try_wait() {
            Ok(Some(status)) => {
                println!(
                    "Client exited with status: {:?} success {}",
                    &status,
                    &status.success()
                );
                wait_with_output("client", client);
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
    wait_with_output("server", server);
    assert!(client_succeeded);
    Ok(())
}

fn do_run_example(example: &str, args: &[&str]) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("run").arg("--example").arg(example);

    if !args.is_empty() {
        cmd.arg("--").args(args);
    }

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir("example");
    cmd
}

fn wait_with_output(name: &str, cmd: Child) {
    if let Ok(output) = cmd.wait_with_output() {
        println!("==== {name} output begin");
        println!("==== stdout:");
        output.stdout.lines().for_each(|line| {
            println!("{}", line.unwrap());
        });
        println!("==== stderr:");
        output.stderr.lines().for_each(|line| {
            println!("{}", line.unwrap());
        });
        println!("==== {name} output end");
    }
}

#[test]
fn run_examples() -> Result<(), Box<dyn std::error::Error>> {
    // Local
    run_example("server", "client", &[])?;
    run_example("async-server", "async-client", &[])?;
    run_example("async-stream-server", "async-stream-client", &[])?;

    // TCP
    #[cfg(not(windows))]
    {
        run_example("server", "client", &["--tcp"])?;
        run_example("async-server", "async-client", &["--tcp"])?;
        run_example("async-stream-server", "async-stream-client", &["--tcp"])?;
    }

    Ok(())
}
