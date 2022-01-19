
from functools import reduce
from random import randint
duration = 100
numOfUser = 10
rewardTokensPerSlot = 1
users = [(0, 0)]*numOfUser
maxDeposit = 20
poolSize = reduce(lambda b, u: b+u[0], users, 0)

with open("tests/randoms", "w") as f:
    f.write("{}\n".format(duration))
    f.write("{}\n".format(numOfUser))
    f.write("{}\n".format(rewardTokensPerSlot*duration))
    for i in range(0, duration):
        j = randint(0, numOfUser-1)
        if i != 0 and randint(0, 4) < 2:
            change = 0
            f.write("0\n")
        else:
            change = 0
            while change == 0:
                change = randint(-users[j][0] + 1, maxDeposit)
            f.write("1 {} {}\n".format(j, change))

        poolSize = reduce(lambda b, u: b+u[0], users, 0)
        for (balance, reward) in users:
            if poolSize != 0:
                assert(balance >= 0)
                reward += balance / poolSize * rewardTokensPerSlot

        users[j] = (users[j][0] + change, users[j][1])
