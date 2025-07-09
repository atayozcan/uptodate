# UpToDate

A modern GNOME system update manager written in Rust.

## Features

- **Modern GNOME UI**: Built with GTK4 and libadwaita for a native GNOME experience
- **Multi-Package Manager Support**: Handles system packages (pacman, apt, dnf, etc.), Flatpak, Snap, and more
- **Async Operations**: Non-blocking UI with real-time progress updates
- **Configuration Management**: Persistent settings with TOML configuration
- **Real-time Logging**: Live output from update operations
- **Dry Run Support**: Test updates without applying changes
- **Custom Commands**: Support for custom update scripts
- **No External Dependencies**: Native implementation without requiring topgrade

## Dependencies

- Rust 1.70+
- GTK4
- libadwaita

## Building

```bash
cargo build --release
```

## Running

```bash
cargo run
```

## Configuration

Configuration is stored in `~/.config/uptodate/config.toml`:

```toml
auto_update = false
update_interval_hours = 24
show_notifications = true
dry_run = false
verbose = false
excluded_packages = []
```

## Supported Package Managers

- **System**: pacman, apt, dnf, zypper, yum, apk
- **Universal**: Flatpak, Snap
- **Programming Languages**: Cargo (Rust), pip/pipx (Python), npm/yarn/pnpm (Node.js), gem (Ruby), composer (PHP), go
- **Development Tools**: VS Code/VSCodium extensions, Rustup
- **Others**: Homebrew, Nix, custom commands

## Architecture

- `src/main.rs`: Application entry point and state management
- `src/ui/`: User interface components
- `src/config/`: Configuration management
- `src/services/`: Update service and package manager implementations
- `resources/`: UI definitions and assets

## License

MIT
