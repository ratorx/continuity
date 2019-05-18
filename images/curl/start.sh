#! /bin/bash
tc qdisc add dev eth0 root tbf rate 40mbit burst 1mbit latency 400ms
curl -o /dev/null -w "\nTTFB: %{time_starttransfer}\nTotal: %{time_total}\n" "$@" 2>&1 | tr $'\r' $'\n'
