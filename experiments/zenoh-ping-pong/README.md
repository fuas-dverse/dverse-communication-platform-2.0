# Simple zenoh ping-pong experiment

## Structure
2 projects:
 - one listens on `/ping`
  - on message received:
   - print the message
   - publish a message with content `{ID} pong` on `/pong`
 - one listens on `/pong`
  - on message received:
   - print the message
   - publish a message with content `{ID} ping` on `/ping`

## To use:

### Setup:
1. run `./setup.sh install-deps`

### Running

1. in terminal 1 run: `cd ping && cargo run`
2. in terminal 2 run: `cd pong && cargo run`
3. in terminal 3 run: `./setup.sh run`
