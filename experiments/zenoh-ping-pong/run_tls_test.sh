#!/bin/bash

# Run ping application with TLS config
echo "Starting ping application with TLS..."
cd ping
cargo run &
PING_PID=$!

# Give ping some time to start
sleep 2

# Run pong application with TLS config  
echo "Starting pong application with TLS..."
cd ../pong
cargo run &
PONG_PID=$!

# Give pong some time to start
sleep 2

# Send initial message using Python
echo "Sending initial message..."
cd ..
python send_starting_message.py

# Wait for a few seconds to see the ping-pong in action
sleep 10

# Kill the processes
kill $PING_PID $PONG_PID 2>/dev/null || true

echo "Test completed"