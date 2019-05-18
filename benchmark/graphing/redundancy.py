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
params = [(3, 3), (2, 4), (1, 5)]
param_desc = {(3, 3): "3 up 3 down", (2, 4): "2 up 4 down", (1, 5): "1 up 5 down"}


class PieceSet:
    def __init__(self, l):
        self.buffer = [0] * l
        self.left = l

    def store(self, i):
        if self.buffer[i] == 0:
            self.left -= 1
        self.buffer[i] += 1

    def get_redundant(self):
        assert self.left == 0
        return sum(self.buffer) - len(self.buffer)


datapoints = []
for item, p in product(categories, params):
    folder_name = f"{item}_{p[0]}_{p[1]}"
    for d in os.scandir(f"{sys.argv[1]}/{folder_name}"):
        if d.name.startswith("run"):
            print(d.name)
            dfs = []
            for peer in os.scandir(d):
                with open(peer) as f:
                    df = pd.read_csv(peer)
                    dfs.append(df)
            piece_frame = pd.concat(dfs)
            piece_frame.sort_values("time", inplace=True)
            ps = PieceSet(1574)
            finish = piece_frame.iloc[-1]["time"]
            for _, row in piece_frame.iterrows():
                ps.store(row["index"])
                if ps.left == 0:
                    datapoints.append((ps.get_redundant(), d.name, param_desc[p], item))
                    break
            assert ps.left == 0


bittorrent = pd.DataFrame(
    datapoints,
    columns=["Number of Redundant Pieces", "run", "Swarm Configuration", "Strategy"],
)

g = sns.catplot(
    data=bittorrent,
    x="Strategy",
    y="Number of Duplicate Pieces",
    hue="Swarm Configuration",
    kind="bar",
)

plt.title("Piece Wastage After Redundancy")
plt.show()
