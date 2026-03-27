#!/bin/bash

# CC Switch 2.0 Provider-Specific Edition - Distribution Package Builder

echo "Building CC Switch 2.0 Provider-Specific Edition Distribution Package..."
echo "======================================================================="

# Create distribution directory
DIST_DIR="ccswitch2.0-provider-specific-distribution"
mkdir -p "$DIST_DIR/src-tauri/src"
mkdir -p "$DIST_DIR/src-tauri/src/commands"
mkdir -p "$DIST_DIR/src-tauri/src/services/provider"
mkdir -p "$DIST_DIR/src/skills"
mkdir -p "$DIST_DIR/src/components"

echo "Copying backend files..."
cp /Users/stark/codes/ccswitch/src-tauri/src/task_dispatcher.rs "$DIST_DIR/src-tauri/src/" 2>/dev/null || echo "  WARNING: task_dispatcher.rs not found"
cp /Users/stark/codes/ccswitch/src-tauri/src/commands/task_dispatcher.rs "$DIST_DIR/src-tauri/src/commands/" 2>/dev/null || echo "  WARNING: commands/task_dispatcher.rs not found"
cp -r /Users/stark/codes/ccswitch/src/skills/task-dispatcher "$DIST_DIR/src/skills/" 2>/dev/null || echo "  WARNING: skills/task-dispatcher not found"

echo "Copying related files..."
cp /Users/stark/codes/ccswitch/src-tauri/src/services/provider/alias.rs "$DIST_DIR/src-tauri/src/services/provider/" 2>/dev/null || echo "  NOTE: alias.rs not found (optional)"

echo "Copying UI components..."
cp /Users/stark/codes/ccswitch/src/components/TaskDispatcher.tsx "$DIST_DIR/src/components/" 2>/dev/null || echo "  NOTE: TaskDispatcher.tsx not found (optional)"

echo "Copying configuration files..."
cp /Users/stark/codes/ccswitch/src-tauri/Cargo.toml "$DIST_DIR/src-tauri/" 2>/dev/null || echo "  WARNING: Cargo.toml not found"
cp /Users/stark/codes/ccswitch/src-tauri/tauri.conf.json "$DIST_DIR/src-tauri/" 2>/dev/null || echo "  WARNING: tauri.conf.json not found"

echo "Copying documentation..."
cp /Users/stark/codes/ccswitch/CC_SWITCH_2.0_PROVIDER_SPECIFIC_EDITION.md "$DIST_DIR/" 2>/dev/null || echo "  WARNING: Main documentation not found"
cp /Users/stark/codes/ccswitch/IMPLEMENTATION_SUMMARY.md "$DIST_DIR/" 2>/dev/null || echo "  WARNING: Implementation summary not found"
cp /Users/stark/codes/ccswitch/dist/ccswitch2.0-provider-specific/README.md "$DIST_DIR/README.md" 2>/dev/null || echo "  NOTE: README.md not found (using default)"

# Create build script
cat > "$DIST_DIR/build.sh" << 'EOF'
#!/bin/bash

# CC Switch 2.0 Provider-Specific Edition Build Script
# Supports manual specification of specific provider IDs for task dispatching

echo "Building CC Switch 2.0 Provider-Specific Edition..."
echo "=================================================="

echo "This version supports:"
echo "- Manual specification of specific provider IDs for task dispatching"
echo "- Precise routing between different providers within Claude models"
echo "- Preserved multi-model support capability"
echo ""

echo "Prerequisites:"
echo "- Rust (1.70+) with Cargo"
echo "- Tauri CLI: cargo install tauri-cli"
echo ""

echo "To build:"
echo "1. cd src-tauri"
echo "2. cargo tauri build"
echo ""

echo "To use in Claude Code:"
echo "- /dispatch-task --target <specific-provider-id> --task '<task>'"
echo ""
echo "Examples:"
echo "- /dispatch-task --target anthropic-us-east --task 'Help me write code'"
echo "- /dispatch-task --target openrouter-meta-llama --task 'Help me analyze text'"
echo "- /dispatch-task --target claude-sonnet-fast --task 'Help me process data'"
EOF

chmod +x "$DIST_DIR/build.sh"

# Create README for distribution if not found
if [ ! -f "$DIST_DIR/README.md" ]; then
cat > "$DIST_DIR/README.md" << 'EOF'
# CC Switch 2.0 Provider-Specific Edition

CC Switch 2.0 Provider-Specific Edition is a specialized application designed for manual specification of specific provider IDs for precise task dispatching.

## Features

- **Precise Provider Routing**: Manually specify exact provider ID for task dispatching
- **Claude Multi-Provider Support**: Enable precise routing between different providers within Claude models
- **Preserved Multi-Model Support**: Still support dispatching across different AI models
- **Validation Mechanism**: Validate that specified providers exist
- **Status Tracking**: Real-time monitoring of task execution status
- **Existing Config Integration**: Direct use of your configured providers in CC Switch without additional setup

## Installation Dependencies

- Rust (1.70+)
- Cargo
- Tauri CLI (install with: `cargo install tauri-cli`)

## Build Steps

```bash
cd src-tauri
cargo tauri build
```

## Usage Methods

Use the following commands in Claude Code for provider-specific task dispatching:

### Specify Claude Provider
```
/dispatch-task --target anthropic-us-east --task "Please help me write a Python function"
```

### Specify Codex Provider
```
/dispatch-task --target openrouter-meta-llama --task "Help me solve this math problem"
```

### Specify Gemini Provider
```
/dispatch-task --target google-gemini-pro --task "Help me generate some text"
```

## Parameter Explanation

- `--target`: Target provider ID (must be a provider configured in CC Switch)
- `--task`: Description of the task to execute
- `--timeout`: Timeout time (seconds), optional

## Check Available Providers

You can check all configured providers with:
```
/get-available-providers
```

## Integration with Existing Configuration

- Direct use of all providers already configured in your CC Switch
- No additional configuration required
- Fully compatible with existing provider settings
- Load balancing will take into account all your providers

## Examples

```
# Dispatch to a specific Claude provider
/dispatch-task --target claude-sonnet-fast --task "Help me optimize this algorithm"

# Dispatch to a specific OpenRouter provider
/dispatch-task --target openrouter-claude-3-opus --task "Help me analyze this code"

# Dispatch to a specific Azure OpenAI provider
/dispatch-task --target azure-gpt4-turbo --task "Help me generate report summary"
```

## Precautions

- Ensure the specified provider ID exists in your CC Switch configuration
- Provider ID is case-sensitive
- System validates whether provider exists, and returns error if not
EOF
fi

echo "Creating source file archive..."
tar -czf "$DIST_DIR-source-files.tar.gz" "$DIST_DIR"

echo ""
echo "Distribution package created successfully!"
echo ""
echo "Package contents:"
echo "- Backend implementation files (Rust)"
echo "- Claude Code skill implementation"
echo "- Build script"
echo "- Complete documentation"
echo "- Usage examples"
echo ""
echo "To use this package:"
echo "1. Extract: tar -xzf $DIST_DIR-source-files.tar.gz"
echo "2. Follow build instructions in the README"
echo "3. Integrate with your CC Switch installation"
echo ""
echo "Directory: $DIST_DIR"
echo "Archive: $DIST_DIR-source-files.tar.gz"