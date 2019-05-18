#! /usr/bin/env python3
import os
import sys
import pandas as pd
import seaborn as sns
import matplotlib.pyplot as plt
from itertools import product

sns.set(style="whitegrid")

datapoints = []

categories = ["bitos", "inorder"]
control = "http"
params = [(3, 3), (2, 4), (1, 5)]
param_desc = {(3, 3): "3 up 3 down", (2, 4): "2 up 4 down", (1, 5): "1 up 5 down"}

datapoints = []
for item, p in product(categories, params):
    folder_name = f"{item}_{p[0]}_{p[1]}"
    for d in os.scandir(f"{sys.argv[1]}/{folder_name}"):
        if d.name.startswith("run"):
            for peer in os.scandir(d):
                with open(peer) as f:
                    df = pd.read_csv(peer)
                    ttfb = df[df["index"] == 0]["time"].iloc[0]
                    datapoints.append((ttfb / 1000, d.name, param_desc[p], item))

for p in params:
    folder_name = f"{control}_{p[0]}_{p[1]}"
    for d in os.scandir(f"{sys.argv[1]}/{folder_name}"):
        if d.name.startswith("run"):
            for peer in os.scandir(d):
                with open(peer) as f:
                    datapoints.append(
                        (
                            float(f.readlines()[-2].split()[-1]),
                            d.name,
                            param_desc[p],
                            control,
                        )
                    )

bittorrent = pd.DataFrame(
    datapoints, columns=["Time Taken (s)", "run", "Swarm Configuration", "Strategy"]
)

g = sns.catplot(
    data=bittorrent,
    x="Strategy",
    y="Time Taken (s)",
    hue="Swarm Configuration",
    kind="bar",
)
g.set(yscale="log")

plt.title("Time to First Byte")
plt.show()
