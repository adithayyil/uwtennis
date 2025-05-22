# uwtennis

A Rust-based monitoring tool that tracks tennis court availability at the University of Waterloo and sends instant notifications when spots open up in specified program levels.

## Configuration
Prerequisites:
- Rust and Cargo (1.70.0 or newer recommended)

Building from source
```shell
# Clone the repository
git clone https://github.com/yourusername/uwtennis.git
cd uwtennis

# Build the project
cargo build --release

# The binary will be available at target/release/uwtennis
```

## Configuration
Create a config.toml file in the root directory with the following structure:

```toml
# How often to check for updates (in seconds)
interval_seconds = 60

# ntfy endpoint - where to send notifications when spots open up
# check ntfy.sh
ntfy_endpoint = ""

# Programs to monitor - add the program IDs you want to track
# You can find program IDs by browsing the UWaterloo rec page
[[program_ids]]
id = "4646d6f1-8319-4b35-bea4-78d0250fc3b8"
name = "Beginner"

[[program_ids]]
id = "98197a06-adb4-4785-b383-e5bd428903a0"
name = "Intermediate"

[[program_ids]]
id = "8f425207-e7a6-44da-8f0f-8adcbf88cedc"
name = "Advanced"
```