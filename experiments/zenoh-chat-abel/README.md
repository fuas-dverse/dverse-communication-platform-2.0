
# Zenoh Chat Experiment (Rust)


A lightweight peer-to-peer terminal chat application built with **Rust**, **Zenoh**, and **Tokio**. This project demonstrates real-time distributed messaging with rate limiting, validation, and async concurrency.

----------

## Features

-   Real-time messaging using Zenoh pub/sub
    
-   Peer-to-peer architecture
    
-   Built with async Rust (Tokio)
    
-   Rate limiting (anti-spam)
    
-   Message validation
    

----------

## Architecture

-   **Protocol:** Zenoh pub/sub
    
-   **Topic:** `chat/global`
    
-   **Runtime:** Tokio
    
-   **Serialization:** JSON (Serde)
    

Each peer:

-   Publishes messages
    
-   Subscribes to the same topic
    
-   Processes messages independently
    
----------

## Installation

### 1. Install Rust

Check if Rust is installed:

```
rustc --version
```

If not, install it:

```
curl https://sh.rustup.rs -sSf | sh
```

Then restart your terminal.

----------

### 2. Clone the project

```
git clone https://github.com/fuas-dverse/dverse-communication-platform-2.0.git
cd dverse-communication-platform-2.0
```

----------

### 3. Build the project

```
cargo build
```


----------

##  Running the App

### Start a peer

```
cargo run
```
----------

###  Run multiple peers

Open **multiple terminals** and run the same command:

```
cargo run
```

Each instance acts as a separate user in the network.

----------

###  Example session

```
Enter username:
abel

[✓] Connected as abel
```

----------

##  Usage

-   Type a message → press **Enter**
-   Messages are broadcast to all connected peers
-   Use `/quit` to exit
----------
##  Message Flow

1.  User inputs message
2.  Message is serialized (JSON)
3.  Published to `chat/global`
4.  All peers receive it
5.  Each peer:
    -   Validates the message
    -   Applies rate limiting
    -   Prints to terminal

----------

##  Safety Features

-    Rejects empty usernames/messages
-    Rejects malformed JSON
-    Prevents spam (3 messages per user per 5 seconds)

----------
##  Project Structure

```
src/
 └── main.rs     # Core chat logic

Cargo.toml       # Dependencies and config
```

----------
##  Tech Stack

-   **Rust** — systems programming
-   **Tokio** — async runtime
-   **Zenoh** — distributed pub/sub
-   **Serde** — serialization
----------

##  Potential Improvements

-    Private channels
-    Persistent message history

----------

##  Why This Project?

This project demonstrates:

-   Distributed system design (P2P)
-   Real-time messaging architecture
-   Safe concurrent Rust programming
-   Practical async patterns

It’s a strong foundation for building:

-   chat systems
-   multiplayer apps
-   distributed tools
