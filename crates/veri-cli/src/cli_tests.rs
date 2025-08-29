#[cfg(test)]
mod tests {
    use crate::cli::{Cli, Commands, Engine, ExitCode};
    use clap::Parser;

    #[test]
    fn test_cli_help_contains_all_flags() {
        let help = Cli::try_parse_from(&["veri", "--help"]).unwrap_err();
        let help_text = help.to_string();
        
        // Check that all required flags are present
        assert!(help_text.contains("--all"));
        assert!(help_text.contains("--watch"));
        assert!(help_text.contains("--keyword"));
        assert!(help_text.contains("--marker"));
        assert!(help_text.contains("--workers"));
        assert!(help_text.contains("--last-failed"));
        assert!(help_text.contains("--junit-xml"));
        assert!(help_text.contains("--jsonl"));
        assert!(help_text.contains("--explain"));
        assert!(help_text.contains("--engine"));
    }

    #[test]
    fn test_exit_codes() {
        assert_eq!(ExitCode::Success as i32, 0);
        assert_eq!(ExitCode::TestFailure as i32, 1);
        assert_eq!(ExitCode::Interrupted as i32, 2);
        assert_eq!(ExitCode::InternalError as i32, 3);
        assert_eq!(ExitCode::UsageError as i32, 4);
    }

    #[test]
    fn test_cli_parsing_basic_flags() {
        let args = Cli::parse_from(&["veri", "--all", "--watch", "--explain"]);
        assert!(args.all);
        assert!(args.watch);
        assert!(args.explain);
    }

    #[test]
    fn test_cli_parsing_with_values() {
        let args = Cli::parse_from(&[
            "veri", 
            "--workers", "8",
            "--keyword", "test_example",
            "--marker", "slow",
            "--junit-xml", "results.xml"
        ]);
        assert_eq!(args.workers, Some("8".to_string()));
        assert_eq!(args.keyword, Some("test_example".to_string()));
        assert_eq!(args.marker, Some("slow".to_string()));
        assert_eq!(args.junit_xml, Some(std::path::PathBuf::from("results.xml")));
    }

    #[test]
    fn test_engine_enum() {
        let args = Cli::parse_from(&["veri", "--engine", "pytest"]);
        assert_eq!(args.engine, Engine::Pytest);
        
        let args = Cli::parse_from(&["veri", "--engine", "veri"]);
        assert_eq!(args.engine, Engine::Veri);
    }

    #[test]
    fn test_subcommands() {
        let args = Cli::parse_from(&["veri", "split", "--ci", "4"]);
        match args.command {
            Some(Commands::Split { shards }) => assert_eq!(shards, 4),
            _ => panic!("Expected Split command"),
        }

        let args = Cli::parse_from(&["veri", "shard", "--ci", "2"]);
        match args.command {
            Some(Commands::Shard { shard_id, manifest: _ }) => assert_eq!(shard_id, 2),
            _ => panic!("Expected Shard command"),
        }
    }

    #[test]
    fn test_verbosity_levels() {
        let args = Cli::parse_from(&["veri", "-v"]);
        assert_eq!(args.verbose, 1);

        let args = Cli::parse_from(&["veri", "-vv"]);
        assert_eq!(args.verbose, 2);

        let args = Cli::parse_from(&["veri", "--quiet"]);
        assert!(args.quiet);
    }
}