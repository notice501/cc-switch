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
