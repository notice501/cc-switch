# CC Switch 2.0 Provider-Specific Edition

## Overview

CC Switch 2.0 Provider-Specific Edition is an enhanced version of CC Switch that enables precise task dispatching to specific AI model providers. Rather than relying on automatic platform selection, users can manually specify exact provider IDs for precise task routing across Claude, Codex, Gemini, and other AI model providers.

## Key Features

- **Precise Provider Routing**: Manually specify exact provider IDs for task dispatch
- **Claude Multi-Provider Support**: Route tasks between different Claude providers with precision
- **Preserved Multi-Model Support**: Still supports dispatching across different AI models
- **Validation Mechanism**: Validates that specified providers exist
- **Real-time Monitoring**: Track task execution status
- **Seamless Integration**: Works with existing CC Switch provider configurations

## Installation and Setup

### 1. Prerequisites

- Rust (1.70+) with Cargo
- Node.js and npm (for the UI components)
- Tauri CLI: `cargo install tauri-cli`

### 2. Building the Application

```bash
cd src-tauri
cargo tauri build
```

## Usage Guide

### Basic Task Dispatch

In Claude Code, use the following commands for provider-specific task dispatching:

#### Specify Claude Provider
```
/dispatch-task --target anthropic-us-east --task "Please help me write a Python function"
```

#### Specify Codex Provider
```
/dispatch-task --target openrouter-meta-llama --task "Help me solve this math problem"
```

#### Specify Gemini Provider
```
/dispatch-task --target google-gemini-pro --task "Help me generate some text"
```

### Command Parameters

- `--target`: Target provider ID (must be a provider configured in CC Switch)
- `--task`: Task description to execute
- `--timeout`: Timeout in seconds (optional)

### Check Available Providers

View all configured providers:
```
/get-available-providers
```

### Check Task Status

Monitor task execution:
```
/get-task-status --task-id <task-id>
```

## Architecture

### Core Components

1. **Task Dispatcher Backend (`src/task_dispatcher.rs`)**:
   - Manages task queues and execution
   - Validates provider existence before routing
   - Tracks task execution status

2. **Tauri Commands (`src/commands/task_dispatcher.rs`)**:
   - Exposes task dispatch functionality to the frontend
   - Provides API endpoints for Claude Code integration

3. **Claude Code Skill (`src/skills/task-dispatcher/`)**:
   - Implements the `/dispatch-task` command in Claude Code
   - Handles command parsing and validation

### Provider Management

The system leverages CC Switch's existing provider management system, allowing you to use all configured providers without additional setup. Each provider is identified by a unique ID and associated with its respective platform.

## Implementation Details

### Provider Validation

Before dispatching any task, the system validates that the specified provider ID exists in any of the configured platforms (Claude, Codex, Gemini, OpenCode). If the provider doesn't exist, an error is returned.

### Task Execution Flow

1. Task submission with specific provider ID
2. Provider validation against existing configurations
3. Task queued with priority
4. Task dispatched to specified provider
5. Execution result captured and stored
6. Status available for querying

## Integration with Existing Configurations

- Uses all providers already configured in CC Switch
- No additional configuration required
- Fully compatible with existing provider setups
- Maintains provider settings and authentication

## Use Cases

### Code Generation Tasks
With multiple Claude providers (e.g., different regional nodes), you can:
```
# Let system choose best Claude provider
/dispatch-task --target claude --task "Write a quicksort in Python"

# Or specify a specific provider
/dispatch-task --target anthropic-us-west --task "Write a quicksort in Python"
```

### Mathematical Computation Tasks
For math-intensive tasks using Codex with automatic selection:
```
/dispatch-task --target codex --task "Calculate matrix eigenvalues"
```

### Content Generation Tasks
For creative writing tasks using Gemini with provider specification:
```
/dispatch-task --target google-gemini-pro --task "Write an article about AI ethics"
```

## Error Handling

- Invalid provider IDs return descriptive error messages
- Empty task content is rejected
- Timeouts are configurable per task
- Failed tasks are logged with error details

## Best Practices

1. **Meaningful Provider Naming**: Use descriptive provider IDs like `anthropic-us-east-production`
2. **Load Monitoring**: Regularly check provider loads to ensure balanced utilization
3. **Diverse Configurations**: Configure providers with different characteristics for various task types
4. **Performance Monitoring**: Monitor success rates and response times across providers

## Troubleshooting

### Task Not Routed as Expected
1. Verify provider is correctly configured
2. Confirm provider is online and accessible
3. Check provider load status

### Performance Issues
1. Check for provider overload
2. Review success rates across providers
3. Consider adding more providers

## Conclusion

The CC Switch 2.0 Provider-Specific Edition delivers the precise task routing capability requested, enabling users to manually specify which AI provider should execute their tasks while maintaining compatibility with existing CC Switch configurations. The system provides intelligent routing, validation, and monitoring capabilities for an enhanced AI task management experience.