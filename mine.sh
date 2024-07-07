#!/bin/bash

#################################
## Begin of user-editable part ##
#################################

POOL=
THREADS=1
USER=addr1q84hm3lxp9ktahnladplgu55lmkyq304gfydqsvgcrs9j4nk30xhzn0cux8eqc74sw4zrh5rgqll02xw32p0sz6mu6qsdny38p.LAPTOP
PASS=x
# Logging level (options: error, warn, info, debug, trace)
LOG_LEVEL=info

#################################
##  End of user-editable part  ##
#################################

# Loop to restart the miner if it crashes
while true; do
    # Run the miner with the specified options
    # Available options:
    #   --pool HOSTPORT    : The pool host and port (required)
    #   --threads NUM      : The number of threads to use (optional)
    #   --user USERNAME    : The username for authentication (required)
    #   --pass PASSWORD    : The password for authentication (optional)
    #   --difficulty NUM   : The custom difficulty to use (optional)
    #   $@                 : Any additional command-line arguments passed to this script
    LOG_LEVEL=$LOG_LEVEL ./fortuna-cpu-miner --pool $POOL --user $USER --pass $PASS --threads $THREADS $@

    if [ $? -ne 0 ]; then
        echo "Program exited with an error. Waiting for 10 seconds before restarting..."
        sleep 10
        echo "Restarting the program..."
    else
        break
    fi
done