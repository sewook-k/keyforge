fn main() -> anyhow::Result<()> {
    let service = keyforge_daemon::AppService::new_default()?;
    let bootstrap = service.bootstrap();
    println!(
        "KeyForge daemon ready at revision {} ({:?})",
        bootstrap.settings.revision, bootstrap.runtime.engine_state
    );
    println!("The desktop app owns the long-running message loop in the MVP build.");
    Ok(())
}
