#!/usr/bin/env bash
set -euo pipefail

CERT_DIR="${CERT_DIR:-./certs}"
CA_DAYS="${CA_DAYS:-3650}"
CERT_DAYS="${CERT_DAYS:-365}"
KEY_SIZE="${KEY_SIZE:-4096}"
SERVER_SAN="${SERVER_SAN:-DNS:zenoh.local}"
CA_CN="${CA_CN:-ZenohCA}"
SERVER_CN="${SERVER_CN:-zenoh-router}"
CLIENT_CN="${CLIENT_CN:-zenoh-client}"

install-deps() {
    python -m venv venv
    source venv/bin/activate
    pip install -r requirements.txt
}

run() {
    source venv/bin/activate
    python send_starting_message.py
}

gen-certs() {
    rm -rf "$CERT_DIR"
    mkdir -p "$CERT_DIR"

    echo "=== Generating CA ==="
    openssl genrsa -out "$CERT_DIR/ca.key" "$KEY_SIZE"
    openssl req -x509 -new -nodes \
        -key "$CERT_DIR/ca.key" \
        -sha256 \
        -days "$CA_DAYS" \
        -out "$CERT_DIR/ca.crt" \
        -subj "/CN=$CA_CN"

    echo "=== Generating server certificate ==="
    openssl genrsa -out "$CERT_DIR/server.key" "$KEY_SIZE"
    openssl req -new \
        -key "$CERT_DIR/server.key" \
        -out "$CERT_DIR/server.csr" \
        -subj "/CN=$SERVER_CN"

    cat > "$CERT_DIR/server_ext.cnf" <<EOF
extendedKeyUsage = serverAuth
subjectAltName = $SERVER_SAN
EOF

    openssl x509 -req \
        -in "$CERT_DIR/server.csr" \
        -CA "$CERT_DIR/ca.crt" \
        -CAkey "$CERT_DIR/ca.key" \
        -CAcreateserial \
        -out "$CERT_DIR/server.crt" \
        -days "$CERT_DAYS" \
        -sha256 \
        -extfile "$CERT_DIR/server_ext.cnf"

    echo "=== Generating client certificate ==="
    openssl genrsa -out "$CERT_DIR/client.key" "$KEY_SIZE"
    openssl req -new \
        -key "$CERT_DIR/client.key" \
        -out "$CERT_DIR/client.csr" \
        -subj "/CN=$CLIENT_CN"

    cat > "$CERT_DIR/client_ext.cnf" <<EOF
extendedKeyUsage = clientAuth
EOF

    openssl x509 -req \
        -in "$CERT_DIR/client.csr" \
        -CA "$CERT_DIR/ca.crt" \
        -CAkey "$CERT_DIR/ca.key" \
        -CAcreateserial \
        -out "$CERT_DIR/client.crt" \
        -days "$CERT_DAYS" \
        -sha256 \
        -extfile "$CERT_DIR/client_ext.cnf"

    rm -f "$CERT_DIR"/*.csr "$CERT_DIR"/*.cnf "$CERT_DIR"/*.srl

    echo ""
    echo "=== Done ==="
    echo "Generated in $CERT_DIR/:"
    echo "  ca.crt, ca.key         - Certificate Authority (CN=$CA_CN)"
    echo "  server.crt, server.key - Router (SAN=$SERVER_SAN)"
    echo "  client.crt, client.key - Client (CN=$CLIENT_CN)"
}

"$@"
