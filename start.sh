#! /bin/sh
tc qdisc add dev eth0 root tbf rate 40mbit burst 1mbit latency 400ms
./target/release/continuity "$@" >/dev/null
