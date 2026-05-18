# Zenoh Chat Experiment (Rust)

A lightweight peer-to-peer terminal chat application built with **Rust**, **Zenoh**, and **Tokio**. This project demonstrates real-time distributed messaging with rate limiting, validation, and async concurrency.

---

## 1. Features

* Real-time messaging using Zenoh pub/sub
* Peer-to-peer architecture (no central server)
* Built with async Rust (Tokio)
* Message validation (safe parsing)
* Rate limiting (3 messages per 5 seconds per user)

---

## 2. Architecture

* **Protocol:** Zenoh pub/sub
* **Topic:** `chat/global`
* **Runtime:** Tokio
* **Serialization:** JSON (Serde)

Each peer:

* Publishes messages
* Subscribes to the same topic
* Processes messages independently

### Message Flow

1. User inputs message
2. Message is serialized (JSON)
3. Published to `chat/global`
4. All peers receive it
5. Each peer:

    * validates message
    * applies rate limiting
    * prints output

---

## 3. Engineering Approach

This project was built incrementally using an engineering approach:

* First implemented message structure and serialization
* Then integrated Zenoh pub/sub communication
* Added asynchronous message handling using Tokio
* Implemented validation and rate limiting
* Iteratively tested behavior using multiple peers

Each step was validated through real execution, ensuring correct distributed behavior.

---

## 4. Design Decisions

### Why peer-to-peer architecture?

A peer-to-peer model was chosen to explore decentralized system design, where each node is responsible for both sending and receiving messages. This removes the need for a central server and distributes system responsibility.

---

### Why Zenoh?

Zenoh was selected because it provides:

* low-latency pub/sub messaging
* support for distributed systems
* simplified peer communication

This allowed focus on system design rather than infrastructure complexity.

---

### Why local rate limiting?

Rate limiting is implemented on each peer instead of a central server because:

* there is no central authority in a P2P system
* each node must independently handle spam prevention
* it demonstrates distributed responsibility in system design

---

### Why Arc + Mutex?

Shared state (message counter and rate limiter) is managed using:

* `Arc` → shared ownership across async tasks
* `Mutex` → safe mutable access

This ensures thread-safe concurrency in an asynchronous runtime.

---

## 5. Software Quality & Validation

* Invalid messages are safely ignored
* Empty usernames/messages are rejected
* Rate limiting prevents message spam
* System remains stable under concurrent peers

The system was tested by running multiple instances and observing message propagation and filtering behavior.

---

## 6. Installation

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

---

### 2. Clone the project

```
git clone https://github.com/fuas-dverse/dverse-communication-platform-2.0.git
cd dverse-communication-platform-2.0
```

---

### 3. Build the project

```
cargo build
```

---

## 7. Running the App

### Start a peer

```
cargo run
```

---

### Run multiple peers

Open multiple terminals and run:

```
cargo run
```

Each instance acts as a separate user in the network.

---

### Example session

```
Enter username:
abel

[✓] Connected as abel
```

---

## 8. Usage

* Type a message → press Enter
* Messages are broadcast to all peers
* Use `/quit` to exit

---

## 9. Safety Features

* Rejects empty usernames/messages
* Rejects malformed JSON
* Prevents spam (3 messages per user per 5 seconds)

---

## 10. Software Maintenance

The system is designed to be maintainable through:

* clear separation of concerns (networking vs logic)
* modular helper functions
* reusable message structure
* simple architecture that can be extended (e.g. authentication, private chat rooms)

---

## 11. Tech Stack

* Rust — systems programming
* Tokio — async runtime
* Zenoh — distributed pub/sub
* Serde — serialization

---

## 12. Potential Improvements

* Private channels
* Persistent message history
* Authentication system (auth_token usage)
* GUI or TUI interface
* Advanced rate limiting (token bucket model)

---

## 13. Why This Project?

This project demonstrates:

* distributed system design (peer-to-peer architecture)
* real-time messaging systems
* safe concurrent programming in Rust
* practical async programming patterns

---

## 14. Reflection

A key insight from this project is how system responsibilities change in a decentralized architecture.

Without a central server, each peer must independently handle validation, rate limiting, and reliability. This required careful design of local state and concurrency management.

This strengthened my understanding of distributed systems and asynchronous programming in Rust.
