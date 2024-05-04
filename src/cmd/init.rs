use anyhow::Result;
use tokio::fs;

pub async fn init(name: &String) -> Result<()> {
    let path_exists = fs::try_exists(name).await?;

    if path_exists {
        eprintln!("The target directory {} already exists.", name);
        std::process::exit(1);
    } else {
        // Create the directory and all its parent directories if required
        fs::create_dir_all(name).await?;
        // Get the canonical (absolute) path to the new site
        let path = fs::canonicalize(name).await?;

        let init_message = format!(
            r#"
Congratulations, your new Norgolith site was created in {}

Please make sure to read the documentation at {}.
            "#,
            path.display(),
            "https://foobar.wip/"
        );
        println!("{}", init_message);
    }

    Ok(())
}
