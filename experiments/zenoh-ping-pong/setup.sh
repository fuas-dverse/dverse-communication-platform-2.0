#!/usr/bin/env bash

install-deps() {
    python -m venv venv
    source venv/bin/activate
    pip install -r requirements.txt
}

run() {
    source venv/bin/activate
    python send_starting_message.py

}

"$@"

