Cairn for Windows
=================

Cairn is a personal desktop app for tasks and knowledge pages, with search by keyword and by
meaning, an Assistant, and a spelling and grammar checker. It works offline; only the Assistant
talks to the AI service you set up. This is the portable version for x64 Windows 11: there is
no installer, no Start menu entry and no uninstaller.

The "lite" zip (its name ends in -lite) is the same without the search model, so it is a
smaller download. Search by words works as usual; search by meaning needs the model, which
Settings > Search can download (about 200 MB) or add from a file.

1. Unzip this folder OUTSIDE OneDrive
-------------------------------------
Unzip it to a folder that is not synced, for example C:\Users\<you>\Cairn (or C:\Cairn).
OneDrive locks and syncs the database files and breaks them. Keep the path short and plain
(no accents or Chinese characters in folder names).
Before unzipping, you can right-click the .zip, choose Properties, and tick Unblock.

2. Start it
-----------
Double-click cairn.exe. The first time, Windows says "Windows protected your PC", because the
program is not code-signed. Click "More info", then "Run anyway". Windows asks again for each
new version. The first start is slow (Microsoft Defender scans the files); later starts are faster.

3. Long paths
-------------
Some files inside this folder have long paths. If check-windows.ps1 (see below) says that long
paths are off and your folder is too deep, either unzip to a shorter folder such as C:\Cairn,
or open PowerShell as administrator, run

    New-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name LongPathsEnabled -Value 1 -PropertyType DWORD -Force

and restart Windows.

4. Where your data is
---------------------
Cairn creates a "data" folder inside this folder the first time it starts. Your tasks, pages,
settings and backups are there. API keys are not in the folder: they go to Windows Credential
Manager. You have to enter them again on each computer.

5. A new version
----------------
A new version comes as a new folder. Do not unzip it over the old one. In the old version use
Settings > Backup & data > "Export everything", then in the new version choose "Import from a backup or export" on
the first start (or in Settings > Backup & data). Your data moves with Export and Import.

6. Checking this computer
-------------------------
If something does not work, or to check a new computer, open PowerShell in this folder and run

    powershell -ExecutionPolicy Bypass -File check-windows.ps1

It takes a few minutes and opens a Cairn window for a few seconds (do not click in it). It prints
one line per item: PASS, WARN, FAIL or INFO. Select all in the window, copy, and paste the report
back. It changes nothing except a throw-away folder in %TEMP% and one test entry in Credential
Manager, which it deletes.

7. Licences
-----------
The file NOTICE in this folder lists the licences of the software that Cairn includes, including
the Gemma Terms of Use for the search model (not in the lite zip, which has no model).
