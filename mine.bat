@echo off

rem #################################
rem ## Begin of user-editable part ##
rem #################################

set POOL=
set THREADS=1
set USER=addr1q84hm3lxp9ktahnladplgu55lmkyq304gfydqsvgcrs9j4nk30xhzn0cux8eqc74sw4zrh5rgqll02xw32p0sz6mu6qsdny38p.LAPTOP
set PASS=x

rem #################################
rem ##  End of user-editable part  ##
rem #################################

:loop
fortuna-cpu-miner.exe --pool %POOL% --user %USER% --pass %PASS% --threads %THREADS% %*

if %ERRORLEVEL% neq 0 (
    echo Program exited with an error. Waiting for 10 seconds before restarting...
    timeout /t 10 /nobreak
    echo Restarting the program...
    goto loop
)