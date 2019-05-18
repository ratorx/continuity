#! /usr/bin/env python3
import subprocess
import time
from datetime import datetime, timedelta
from itertools import product


def docker_compose(name, *args):
    cmd = ["docker-compose", "-f", f"cfg/{name}.yml", "--project-directory", "."]
    cmd.extend(args)
    print(cmd)
    return cmd


def benchmark(num, name):
    subprocess.check_call(["mkdir", "-p", f"data/{name}/run{num}"])
    subprocess.check_call(docker_compose(name, "up", "-d"))
    start = datetime.now()
    delta = timedelta(minutes=15)

    # Loop until termination with a timeout
    while True:
        output = subprocess.check_output(docker_compose(name, "ps")).decode("utf-8")
        lines = output.split("\n")
        lines = [line for line in lines if "peer" in line]  # Get peers
        if all(("Exit 0" in line for line in lines)):
            break
        if datetime.now() - start >= delta:
            print("Timeout")
            break
        time.sleep(5)

    # Terminate the simulation in the background
    stop = subprocess.Popen(docker_compose(name, "stop"))

    # Process log output
    output = subprocess.check_output(docker_compose(name, "ps")).decode("utf-8")
    peers = [
        line.split()[0].split("_")[1] for line in output.split("\n") if "Exit 0" in line
    ]
    if "http" in name:
        for peer in peers:
            logs = (
                subprocess.check_output(docker_compose(name, "logs", peer))
                .decode("utf-8")
                .split("\n")
            )
            with open(f"data/{name}/run{num}/{peer}", "w") as f:
                f.write(logs)
                f.flush()
    else:
        for peer in peers:
            logs = "\n".join(
                [
                    ",".join(line.split()[-2:])
                    for line in subprocess.check_output(
                        docker_compose(name, "logs", peer)
                    )
                    .decode("utf-8")
                    .split("\n")
                    if "Datapoint" in line
                ]
            )
            with open(f"data/{name}/run{num}/{peer}.csv", "w") as f:
                f.write("index,time\n")
                f.write(logs)
                f.flush()
    stop.wait()
    subprocess.check_call(docker_compose(name, "rm", "-fs"))


def main():
    categories = ["rarest", "http", "inorder", "bitos"]
    params = [(3, 3), (2, 4), (1, 5)]
    benchmarks = [
        f"{category}_p[0]_p[1]" for category, p in product(categories, params)
    ]
    for i in range(25):
        for name in benchmarks:
            benchmark(i, name)


if __name__ == "__main__":
    main()
