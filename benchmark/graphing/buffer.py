#! /usr/bin/env python3
import os
import sys
import pandas as pd
import seaborn as sns
import matplotlib.pyplot as plt
from itertools import product
from math import ceil
from datetime import datetime as dtime
from datetime import timedelta

sns.set(style="whitegrid")

datapoints = []

categories = ["bitos", "inorder"]
control = "http"
params = [(1, 5), (2, 4), (3, 3)]
param_desc = {(3, 3): "3 up 3 down", (2, 4): "2 up 4 down", (1, 5): "1 up 5 down"}


def gen_cumulative(df, piece_size):
    pending = set()
    req = 0
    cumulative = {0: 0}
    total = 0
    for _, row in df.iterrows():
        index = row["index"]
        time = row["time"]
        if index == req:
            req += 1
            acked = 1
            while req in pending:
                pending.remove(req)
                req += 1
                acked += 1
            total += acked * piece_size
            cumulative[ceil(time / 1000)] = total
        else:
            pending.add(index)
    gen = pd.DataFrame(cumulative.items(), columns=["time", "downloaded"])
    return gen


def calculate_buffer_time_for_bittorrent(cumulative_download, lag=0):
    rate = 2.5 * 1024 * 1024

    def get_required(t, l):
        return min(rate * max((t - l), 0), 412466670)

    def time_to_get(req):
        return cumulative_download[cumulative_download["downloaded"] >= required].iloc[
            0
        ]["time"]

    for _, row in cumulative_download.iterrows():
        time = row["time"]
        downloaded = row["downloaded"]
        required = get_required(time, lag)
        if required > downloaded:
            lag += time_to_get(required) - time

    return lag


datapoints = []
for item, p in product(categories, params):
    folder_name = f"{item}_{p[0]}_{p[1]}"
    for d in os.scandir(f"{sys.argv[1]}/{folder_name}"):
        if d.name.startswith("run"):
            print(d.name)
            for peer in os.scandir(d):
                with open(peer) as f:
                    # print(peer)
                    df = pd.read_csv(peer)
                    ttfb = df[df["index"] == 0]["time"].iloc[0] / 1000
                    c = gen_cumulative(df, 262144)
                    lag = calculate_buffer_time_for_bittorrent(c, ttfb)
                    assert lag >= ttfb
                    datapoints.append((lag - ttfb, d.name, param_desc[p], item))


def to_bytes(s):
    if s.endswith("k"):
        return int(float(s[:-1]) * 1024)
    elif s.endswith("M"):
        return int(float(s[:-1]) * 1024 * 1024)
    else:
        return int(s)


for p in params:
    folder_name = f"{control}_{p[0]}_{p[1]}"
    for d in os.scandir(f"{sys.argv[1]}/{folder_name}"):
        if d.name.startswith("run"):
            for peer in os.scandir(d):
                with open(peer) as f:
                    lines = f.readlines()
                    ttfb = float(lines[-2].split()[-1])
                    total = float(lines[-1].split()[-1])
                    splits = (
                        line.split()
                        for line in lines
                        if ":" in line and "--:--:--" not in line and "curl" not in line
                    )
                    df = []
                    for sp in splits:
                        if len(sp) < 14:
                            continue
                        t = dtime.strptime(f"0{sp[-3]}", "%H:%M:%S")
                        df.append(
                            (
                                timedelta(
                                    hours=t.hour, minutes=t.minute, seconds=t.second
                                ).total_seconds(),
                                to_bytes(sp[5]),
                            )
                        )
                    df.append((total, 412466670))
                    df = pd.DataFrame(df, columns=["time", "downloaded"])
                    lag = calculate_buffer_time_for_bittorrent(df, ttfb)
                    assert lag >= ttfb
                    datapoints.append((lag - ttfb, d.name, param_desc[p], control))

df = pd.DataFrame(
    datapoints, columns=["Lag (s)", "run", "Swarm Configuration", "Strategy"]
)

g = sns.catplot(
    data=df, x="Strategy", y="Lag (s)", hue="Swarm Configuration", kind="bar"
)

plt.title("Estimated Lag Time After Video Start")
plt.show()
