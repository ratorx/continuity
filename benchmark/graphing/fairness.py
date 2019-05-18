#! /usr/bin/env python3
import os
import sys
import pandas as pd
import seaborn as sns
import matplotlib.pyplot as plt
from itertools import product

sns.set(style="whitegrid")

datapoints = []

categories = ["bitos", "inorder", "rarest"]
control = "http"
params = [(3, 3), (2, 4), (1, 5)]
param_desc = {(3, 3): "3 up 3 down", (2, 4): "2 up 4 down", (1, 5): "1 up 5 down"}


datapoints = []
for item, p in product(categories, params):
    folder_name = f"{item}_{p[0]}_{p[1]}"
    for d in os.scandir(f"{sys.argv[1]}/{folder_name}"):
        if d.name.startswith("run"):
            completions = []
            for peer in os.scandir(d):
                with open(peer) as f:
                    completions.append(pd.read_csv(peer).iloc[-1]["time"])

            datapoints.append(
                (
                    (max(completions) - min(completions)) / 1000,
                    d.name,
                    param_desc[p],
                    item,
                )
            )

for p in params:
    folder_name = f"{control}_{p[0]}_{p[1]}"
    for d in os.scandir(f"{sys.argv[1]}/{folder_name}"):
        if d.name.startswith("run"):
            completions = []
            for peer in os.scandir(d):
                with open(peer) as f:
                    completions.append(float(f.readlines()[-1].split()[-1]))

            datapoints.append(
                (max(completions) - min(completions), d.name, param_desc[p], control)
            )


bittorrent = pd.DataFrame(
    datapoints,
    columns=["Time Difference (s)", "run", "Swarm Configuration", "Strategy"],
)

g = sns.catplot(
    data=bittorrent,
    x="Strategy",
    y="Time Difference (s)",
    hue="Swarm Configuration",
    kind="bar",
)

plt.title("Completion Time Difference")
plt.show()
