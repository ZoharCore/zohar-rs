use std::path::PathBuf;

use zohar_content::ContentRuntimeBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let builder = if let Some(path) = args.get(1) {
        ContentRuntimeBuilder::new().db_path(PathBuf::from(path))
    } else {
        ContentRuntimeBuilder::new()
    };

    let runtime = builder.run().await?;

    println!("content runtime ready");
    println!("maps: {}", runtime.catalog().maps.len());
    println!("town_spawns: {}", runtime.catalog().town_spawns.len());
    println!("mobs: {}", runtime.catalog().mobs.len());
    println!("mob_groups: {}", runtime.catalog().mob_groups.len());
    println!(
        "mob_group_groups: {}",
        runtime.catalog().mob_group_groups.len()
    );
    println!("spawn_rules: {}", runtime.catalog().spawn_rules.len());
    println!("motion_entries: {}", runtime.catalog().motion.len());
    println!(
        "mob_chat_strategies: {}",
        runtime.catalog().mob_chat_strategies.len()
    );
    println!("mob_chat_lines: {}", runtime.catalog().mob_chat_lines.len());
    println!(
        "schema_migrations_applied: {}",
        runtime.migration_summary().schema_applied.len()
    );
    println!(
        "data_migrations_applied: {}",
        runtime.migration_summary().data_applied.len()
    );
    println!(
        "rejected_statements: {}",
        runtime.migration_summary().rejected_statements.len()
    );

    Ok(())
}
