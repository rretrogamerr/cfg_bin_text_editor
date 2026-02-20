@echo off
setlocal enabledelayedexpansion

set "CBTE=cfg_bin_text_editor.exe"

if "%~1"=="" goto :usage
if "%~2"=="" goto :usage

set "MODE=%~1"
set "FOLDER=%~2"

if not exist "%FOLDER%" (
    echo Error: Folder "%FOLDER%" does not exist.
    exit /b 1
)

if "%MODE%"=="-e" goto :extract
if "%MODE%"=="-w" goto :write
goto :usage

:extract
echo Counting cfg.bin files...
set "TOTAL=0"
for /r "%FOLDER%" %%f in (*.cfg.bin) do set /a TOTAL+=1

if %TOTAL%==0 (
    echo No cfg.bin files found in "%FOLDER%".
    exit /b 0
)

echo Found %TOTAL% cfg.bin files.
echo.

set "COUNT=0"
set "FAIL=0"
for /r "%FOLDER%" %%f in (*.cfg.bin) do call :do_extract "%%f"
goto :done

:do_extract
set /a COUNT+=1
set /a PERCENT=COUNT*100/TOTAL
set "CFG_FILE=%~1"
echo [!COUNT!/%TOTAL%] !PERCENT!%% Extracting nnk: !CFG_FILE!
"%CBTE%" -e "!CFG_FILE!" --mode nnk --extract-format txt
if errorlevel 1 (
    echo   FAILED: !CFG_FILE!
    set /a FAIL+=1
)
goto :eof

:write
echo Counting json files...
set "TOTAL=0"
for /r "%FOLDER%" %%f in (*.cfg.bin.json) do set /a TOTAL+=1

if %TOTAL%==0 (
    echo No cfg.bin.json files found in "%FOLDER%".
    exit /b 0
)

echo Found %TOTAL% json files.
echo.

set "COUNT=0"
set "FAIL=0"
for /r "%FOLDER%" %%f in (*.cfg.bin.json) do call :do_write "%%f"
goto :done

:do_write
set /a COUNT+=1
set /a PERCENT=COUNT*100/TOTAL
set "JSON=%~1"
set "CFG=!JSON:.json=!"
if exist "!CFG!" (
    echo [!COUNT!/%TOTAL%] !PERCENT!%% Updating nnk: !CFG!
    "%CBTE%" -w "!CFG!" "!JSON!" --mode nnk
    if errorlevel 1 (
        echo   FAILED: !CFG!
        set /a FAIL+=1
    )
) else (
    echo [!COUNT!/%TOTAL%] !PERCENT!%% Skipped: !JSON!
    set /a FAIL+=1
)
goto :eof

:done
echo.
echo Done. !COUNT! files processed, !FAIL! failed.
goto :eof

:usage
echo Usage:
echo   cbte_bulk_nnk.bat -e ^<folder^>    Extract all cfg.bin to txt line-by-line (nnk mode)
echo   cbte_bulk_nnk.bat -w ^<folder^>    Update all cfg.bin from json (nnk mode)
exit /b 1
