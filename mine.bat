@echo off

rem #################################
rem ## Begin of user-editable part ##
rem #################################

set POOL=
set THREADS=1
set USER=addr1q84hm3lxp9ktahnladplgu55lmkyq304gfydqsvgcrs9j4nk30xhzn0cux8eqc74sw4zrh5rgqll02xw32p0sz6mu6qsdny38p.LAPTOP
set PASS=x

rem Logging level (options: error, warn, info, debug, trace)
set LOG_LEVEL=info

rem #################################
rem ##  End of user-editable part  ##
rem #################################

rem Loop to restart the miner if it crashes
:loop

rem Run the miner with the specified options
rem Available options:
rem   --pool HOSTPORT    : The pool host and port (required)
rem   --threads NUM      : The number of threads to use (optional)
rem   --user USERNAME    : The username for authentication (required)
rem   --pass PASSWORD    : The password for authentication (optional)
rem   --difficulty NUM   : The custom difficulty to use (optional)
rem   %*                 : Any additional command-line arguments passed to this script
set LOG_LEVEL=%LOG_LEVEL% && fortuna-cpu-miner.exe --pool %POOL% --user %USER% --pass %PASS% --threads %THREADS% %*

if %ERRORLEVEL% neq 0 (
    echo Program exited with an error. Waiting for 10 seconds before restarting...
    timeout /t 10 /nobreak
    echo Restarting the program...
    goto loop
)