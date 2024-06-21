#!/bin/bash

#################################
## Begin of user-editable part ##
#################################

POOL=
THREADS=1
USER=addr1q84hm3lxp9ktahnladplgu55lmkyq304gfydqsvgcrs9j4nk30xhzn0cux8eqc74sw4zrh5rgqll02xw32p0sz6mu6qsdny38p.LAPTOP
PASS=x

#################################
##  End of user-editable part  ##
#################################

while true; do
    ./fortuna-miner --pool $POOL --user $USER --pass $PASS --threads $THREADS $@

    if [ $? -ne 0 ]; then
        echo "Program exited with an error. Waiting for 10 seconds before restarting..."
        sleep 10
        echo "Restarting the program..."
    else
        break
    fi
done