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

### Things to install yourself

1. cargo/rust
2. zenohd, this might work for you: `cargo install zenohd`
3. get `avahi` or set up a hosts entry to resolve `zenoh.local` to an IP that zenoh is bound to and can listen to.

#### Avahi config
```
[server]
host-name=zenoh
use-ipv4=yes
use-ipv6=yes
allow-interfaces=wlan0,eth0,lo #set to your interfaces, NOTE: "lo" is important as this is what allows you to resolve to "127.0.01" and run the whole thing on one machine.
````

### Setup:
1. run `./setup.sh install-deps`
2. run `./setup.sh gen-certs`

### Running

#### Everything running with TLS

1. in terminal 1 run: `zenohd -c counter-config.json5`
2. in terminal 2 run: `cd ping && CONFIG_FILE=config/tls_config.json5 cargo run`
3. in terminal 3 run: `cd pong && CONFIG_FILE=config/tls_config.json5 cargo run`
4. in terminal 4 run: `CONFIG_FILE=start.json5 ./setup.sh run`

You should see text on the console, meaning there is data exchange between the ping and pong nodes.


#### Ping-Pong running with TLS but start with no TLS


1. in terminal 1 run: `zenohd -c counter-config.json5`
2. in terminal 2 run: `cd ping && CONFIG_FILE=config/tls_config.json5 cargo run`
3. in terminal 3 run: `cd pong && CONFIG_FILE=config/tls_config.json5 cargo run`
4. in terminal 4 run: `./setup.sh run`

Now you should not see any text coming from the ping and pong nodes. meaning no messages are exchanged.

#### TLS not present in a part of the chain

If TLS is not present in any part of the chain (omitting the -c flag on `1.`, or the `CONFIG_FILE` env variable, the demo should break.)
