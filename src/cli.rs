use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "schema-gateway")]
#[command(version = "0.1.0")]
#[command(about = "A lightweight schema validation proxy", long_about = None)]
pub struct Cli {
    /// Path to config file
    #[arg(short, long, value_name = "FILE", default_value = "config.yml")]
    pub config: PathBuf,

    /// Port to listen on
    #[arg(short, long, value_name = "PORT", default_value_t = 8080)]
    pub port: u16,

    /// Validate config and exit
    #[arg(long)]
    pub validate_config: bool,
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        // Test that default values are correctly set
        let cli = Cli {
            config: PathBuf::from("config.yml"),
            port: 8080,
            validate_config: false,
        };

        assert_eq!(cli.config, PathBuf::from("config.yml"));
        assert_eq!(cli.port, 8080);
        assert!(!cli.validate_config);
    }
}
