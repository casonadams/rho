use super::*;

#[tokio::test]
#[cfg(unix)]
async fn cancelling_a_turn_kills_tool_jobs_but_leaves_session_servers() {
    let registry = Registry::default();
    let spawn = |scope| {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 31 & wait");
        isolate_group(&mut cmd);
        let child = cmd.spawn().expect("spawn test shell");
        let pid = child.id().expect("pid");
        registry.track(pid, scope);
        (child, pid)
    };
    let (mut job, job_pid) = spawn(Scope::Turn);
    let (mut server, server_pid) = spawn(Scope::Session);

    registry.kill_turn();

    job.wait().await.expect("reap job");
    wait_group_dead(job_pid).await;
    assert!(!registry.lock().contains_key(&job_pid));
    assert!(registry.lock().contains_key(&server_pid));
    assert!(
        unsafe { libc::kill(-(server_pid as libc::pid_t), 0) } == 0,
        "server group must survive"
    );
    registry.kill_all();
    server.wait().await.expect("reap server");
    wait_group_dead(server_pid).await;
}

#[tokio::test]
#[cfg(unix)]
async fn a_poisoned_registry_still_kills_tracked_groups() {
    let registry = Registry::default();
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("sleep 30 & wait");
    isolate_group(&mut cmd);
    let mut child = cmd.spawn().expect("spawn test shell");
    let pid = child.id().expect("child pid");
    registry.track(pid, Scope::Turn);

    std::thread::scope(|scope| {
        let poisoner = scope.spawn(|| {
            let _held = registry.lock();
            panic!("poison the tracked pid registry");
        });
        assert!(poisoner.join().is_err());
    });
    assert!(registry.pids.is_poisoned());

    registry.kill_all();

    child.wait().await.expect("reap child");
    wait_group_dead(pid).await;
}

#[tokio::test]
#[cfg(unix)]
async fn guard_drop_terminates_the_whole_group() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("sleep 30 & wait");
    isolate_group(&mut cmd);
    let child = cmd.spawn().expect("spawn test shell");
    let pid = child.id().expect("child pid");

    let guard = ProcessTreeGuard::new(child);
    assert_eq!(guard.id(), Some(pid));

    drop(guard);

    wait_group_dead(pid).await;
}

#[tokio::test]
#[cfg(unix)]
async fn guard_kill_and_wait_reaps_child() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("sleep 30 & wait");
    isolate_group(&mut cmd);
    let child = cmd.spawn().expect("spawn test shell");
    let pid = child.id().expect("child pid");

    let mut guard = ProcessTreeGuard::new(child);
    let status = guard.kill_and_wait().await.expect("kill and wait succeeds");
    assert!(!status.success());

    wait_group_dead(pid).await;
}

#[tokio::test]
async fn guard_wait_untracks_pid_on_normal_exit() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("exit 0");
    isolate_group(&mut cmd);
    let child = cmd.spawn().expect("spawn test shell");
    let pid = child.id().expect("child pid");

    let mut guard = ProcessTreeGuard::new(child);
    assert_eq!(guard.id(), Some(pid));

    let status = guard.wait().await.expect("wait succeeds");
    assert!(status.success());
    assert_eq!(guard.id(), None);
}

#[tokio::test]
#[cfg(unix)]
async fn drain_tasks_within_stops_waiting_for_a_grandchild_holding_the_pipes() {
    let cmd = configure_shell_command("sleep 30 & echo started", None);
    let (mut running, mut rx) = StreamingCommand::spawn(cmd).expect("spawn test shell");
    running.wait().await.expect("shell exits");

    running.drain_tasks_within(std::time::Duration::from_millis(50)).await;

    let mut output = String::new();
    while let Ok(chunk) = rx.try_recv() {
        output.push_str(&chunk);
    }
    assert!(output.contains("started"));
}

#[tokio::test]
#[cfg(unix)]
async fn a_drain_timeout_kills_the_grandchild_that_held_the_pipes() {
    let cmd = configure_shell_command("sleep 30 & echo started", None);
    let (mut running, _rx) = StreamingCommand::spawn(cmd).expect("spawn test shell");
    let group = running.group_pid.expect("spawned process has a pid");
    running.wait().await.expect("shell exits");
    assert_eq!(
        unsafe { libc::kill(-(group as libc::pid_t), 0) },
        0,
        "grandchild is alive"
    );

    running.drain_tasks_within(std::time::Duration::from_millis(50)).await;

    wait_group_dead(group).await;
}

#[tokio::test]
#[cfg(unix)]
async fn background_work_that_released_the_pipes_survives_normal_completion() {
    let cmd = configure_shell_command("sleep 31 >/dev/null 2>&1 & echo started", None);
    let (mut running, _rx) = StreamingCommand::spawn(cmd).expect("spawn test shell");
    let group = running.group_pid.expect("spawned process has a pid");
    running.wait().await.expect("shell exits");

    running.drain_tasks_within(std::time::Duration::from_secs(2)).await;

    assert_eq!(unsafe { libc::kill(-(group as libc::pid_t), 0) }, 0);
    kill_group_detached(group);
    wait_group_dead(group).await;
}

#[tokio::test]
#[cfg(unix)]
async fn isolated_child_leads_its_own_session_without_a_controlling_terminal() {
    let mut cmd = Command::new("sleep");
    cmd.arg("30");
    isolate_group(&mut cmd);
    let child = cmd.spawn().expect("spawn sleep");
    let pid = child.id().expect("child pid");

    let sid = unsafe { libc::getsid(pid as libc::pid_t) };
    assert_eq!(sid, pid as libc::pid_t);

    drop(ProcessTreeGuard::new(child));
    wait_group_dead(pid).await;
}

#[tokio::test]
#[cfg(unix)]
async fn isolated_child_cannot_open_the_controlling_terminal() {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("read line < /dev/tty");
    isolate_group(&mut cmd);
    let child = cmd.spawn().expect("spawn test shell");

    let mut guard = ProcessTreeGuard::new(child);
    let status = tokio::time::timeout(std::time::Duration::from_secs(2), guard.wait())
        .await
        .expect("tty read must fail immediately instead of stopping the child")
        .expect("wait succeeds");
    assert!(!status.success());
}

#[test]
#[cfg(unix)]
fn a_child_caught_between_fork_and_exec_does_not_keep_a_session_locked() {
    use rho_harness_core::session::SessionManager;
    use std::os::unix::process::CommandExt;

    fn pipe() -> [libc::c_int; 2] {
        let mut fds = [0; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        fds
    }
    let dir = crate::test_support::temp_dir("fork-window");
    let session_id = SessionManager::new(&dir, None).unwrap().session_id.clone();
    let holder = SessionManager::new(&dir, Some(&session_id)).unwrap();
    let [forked_rd, forked_wr] = pipe();
    let [release_rd, release_wr] = pipe();

    let spawner = std::thread::spawn(move || {
        let mut cmd = std::process::Command::new("true");
        unsafe {
            cmd.pre_exec(move || {
                let byte = 1u8;
                libc::write(forked_wr, (&raw const byte).cast(), 1);
                let mut sink = 0u8;
                libc::read(release_rd, (&raw mut sink).cast(), 1);
                Ok(())
            });
        }
        cmd.status().unwrap()
    });
    let mut byte = 0u8;
    assert_eq!(unsafe { libc::read(forked_rd, (&raw mut byte).cast(), 1) }, 1);

    drop(holder);
    let reopened = SessionManager::new(&dir, Some(&session_id));

    assert_eq!(unsafe { libc::write(release_wr, (&raw const byte).cast(), 1) }, 1);
    assert!(spawner.join().unwrap().success());
    assert!(reopened.is_ok(), "{:?}", reopened.err());
}
