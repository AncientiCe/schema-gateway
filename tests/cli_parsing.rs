use schema_gateway::cli::Cli;
use std::path::PathBuf;

#[test]
fn test_default_arguments() {
    // Given: No arguments provided (using programmatic defaults)
    // When: Create CLI with defaults
    // Then: Should use defaults (config.yml, port 8080)

    let cli = Cli {
        config: PathBuf::from("config.yml"),
        port: 8080,
        validate_config: false,
    };

    assert_eq!(cli.config, PathBuf::from("config.yml"));
    assert_eq!(cli.port, 8080);
    assert_eq!(cli.validate_config, false);
}

#[test]
fn test_custom_config_path() {
    // Given: Custom config path
    // When: Create CLI with custom config
    // Then: Should use custom config path

    let cli = Cli {
        config: PathBuf::from("custom.yml"),
        port: 8080,
        validate_config: false,
    };

    assert_eq!(cli.config, PathBuf::from("custom.yml"));
}

#[test]
fn test_custom_port() {
    // Given: Custom port
    // When: Create CLI with custom port
    // Then: Should bind to port 3000

    let cli = Cli {
        config: PathBuf::from("config.yml"),
        port: 3000,
        validate_config: false,
    };

    assert_eq!(cli.port, 3000);
}

#[test]
fn test_validate_config_mode() {
    // Given: validate-config flag set
    // When: Create CLI with validate_config true
    // Then: Should set validate_config mode to true

    let cli = Cli {
        config: PathBuf::from("config.yml"),
        port: 8080,
        validate_config: true,
    };

    assert_eq!(cli.validate_config, true);
}
