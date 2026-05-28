#!/bin/bash
# RustyClaw Model-Specific Runners Helper

# Detect executable location (prefer release, fallback to debug)
BINARY="/home/kazuaki/Projects/RustyClaw/target/release/rustyclaw-cli"
if [ ! -f "$BINARY" ]; then
    BINARY="/home/kazuaki/Projects/RustyClaw/target/debug/rustyclaw-cli"
fi

# 1. Llama 3.3 70B (High Reasoning)
function claw-llama() {
    RUSTYCLAW_MODEL_NAME="@cf/meta/llama-3.3-70b-instruct-fp8-fast" "$BINARY" "$@"
}

# 2. Qwen 2.5 Coder 32B (Coding & MCP/Tool Calling)
function claw-qwen() {
    RUSTYCLAW_MODEL_NAME="@cf/qwen/qwen2.5-coder-32b-instruct" "$BINARY" "$@"
}

# 3. Llama 3.2 3B (Ultra Fast & Low-Cost)
function claw-speed() {
    RUSTYCLAW_MODEL_NAME="@cf/meta/llama-3.2-3b-instruct" "$BINARY" "$@"
}

# Export functions for interactive shell access
export -f claw-llama
export -f claw-qwen
export -f claw-speed

echo "--------------------------------------------------------"
echo "🦖 RustyClaw Custom Runners Loaded!"
echo "--------------------------------------------------------"
echo "  claw-llama <args>  -> Llama 3.3 70B (High Reasoning)"
echo "  claw-qwen  <args>  -> Qwen 2.5 Coder 32B (Coding & MCP/Tools)"
echo "  claw-speed <args>  -> Llama 3.2 3B (Ultra Fast & Light)"
echo "--------------------------------------------------------"
echo "Example: claw-qwen agent -m 'カレンダーを取得して'"
echo "--------------------------------------------------------"
