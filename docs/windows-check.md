# Windows check (M8, D-59)

The owner's click-through on Windows 11, after `check-windows.ps1` has run. M8 is done when the script passes and every box below is ticked (or its problem is written down). Use the real app in the unzipped folder (outside OneDrive), not the script's throw-away data.

Write what you saw next to anything that failed: what you did, what you expected, what happened, and the folder `data\logs` if there is one.

## Before you start

- [ ] `powershell -ExecutionPolicy Bypass -File check-windows.ps1` ran and the report was pasted back (FAIL lines: none; WARN lines understood).
- [ ] The Cairn folder is outside OneDrive and its path is plain (no accents or Chinese characters).
- [ ] Double-clicking `cairn.exe` shows *Windows protected your PC*; *More info* > *Run anyway* starts it (expected: the program is not signed).

## First start

- [ ] The window opens. Note how long it took the first time (Defender scans the files): ______ s. The second start: ______ s.
- [ ] Home shows the first-start card (*Start fresh* / *Import from a backup or export*) and the folder now holds a `data\` folder next to `cairn.exe`. Nothing was created in `%APPDATA%` for the data (WebView2's own cache in `%LOCALAPPDATA%\app.cairn.desktop` is expected).
- [ ] *Start fresh* works. Closing the window with the X button ends the program (Task Manager shows no `cairn.exe` and no `python.exe` left over after a few seconds).

## Tasks and pages

- [ ] Create a task (title, priority, due date). It shows in the list and on Home; Ctrl+K finds it by a word of its title.
- [ ] Create a knowledge page. Type a misspelt word: a red underline appears within a second or two, and the suggestion replaces the word.
- [ ] Page text with a table, a code block and a link to a task (`#KEY`) renders correctly; reopening the page shows the same text.
- [ ] **A pasted image:** copy an image (a screenshot with Win+Shift+S, or from a browser) and paste it (Ctrl+V) into a page in the editor. It shows in the page and still shows after closing and reopening the page. The file is in `data\` (an `attachments` folder or similar, inside the Cairn folder).
- [ ] **Chinese typing:** with the Microsoft Pinyin input method, type a sentence into a page title, a page body and a task title. The candidate window appears at the cursor, the text is kept as typed, and the font looks right (Microsoft YaHei). Search for a Chinese word (Ctrl+K) and find the page.
- [ ] Switching light and dark theme in Windows changes Cairn's theme (if it is set to follow the system).

## Assistant with Azure sign-in

- [ ] `az login` works in a terminal on this computer (the check script reports whether `az` is signed in).
- [ ] In Settings > AI providers add the Azure provider using the Azure CLI sign-in (no key); *Test connection* succeeds. (The API key, if you use one instead, goes to Credential Manager: Control Panel > Credential Manager > Windows Credentials shows an entry for Cairn.)
- [ ] An Assistant question gets an answer. Note the time to the first words: ______ s. The first answer after a start may be slower because the AI worker starts first.
- [ ] Search by meaning: Ctrl+K with a question-like query finds a related page that shares no words with it (this starts the search model; note the first-use time: ______ s).
- [ ] After closing Cairn, no `python.exe` of the AI worker is left in Task Manager.

## Moving data and backups

- [ ] **Import from Linux:** on the Linux machine, Settings > Backup & data > *Export everything*; copy the `.zip` to Windows. On Windows, on the first start (or Settings > Backup & data) *Import from a backup or export*. Tasks, pages (also Chinese ones), pasted images and settings arrive. Enter the API keys again.
- [ ] After the import, a backup of the previous state was made first, and the backup folder is still this computer's (inside `data\`).
- [ ] **A backup:** the backup card shows a recent backup, with this computer's name in the file name. *Back up now* makes a new one; the file is in the folder shown. Restoring from it (into a throw-away copy of the folder) works.
- [ ] *Export everything* on Windows writes a `.zip` that Linux can import.

## Other Windows risks

- [ ] **The AI worker's first start:** after a fresh start, the first Assistant question or search by meaning waits for the worker. Note how long it took the first time in a session: ______ s (Defender may scan the Python files on the very first use).
- [ ] **Files from Explorer:** drag a file (a `.docx` or `.pdf`) from Explorer onto the window or use *Create knowledge from a file*; the file is read. (Drag and drop is switched off in the window by design: if dropping does nothing, use the file button, and tick this box if the button works.)
- [ ] **Opening folders:** *Open logs folder* (Settings > Backup & data) opens an Explorer window on `data\logs`; links to a web page open the default browser.
- [ ] **Paths with spaces or Chinese characters:** attach a file whose name has a space and a Chinese character; it is imported and shown with the right name.
- [ ] **Window behaviour:** minimise, maximise, resize to the smallest size, move to a second monitor or change the display scaling to 150 %: nothing is cut off or blurry.
- [ ] **Two copies:** starting `cairn.exe` a second time does not damage the data (it either focuses the first window or says it is already running).
- [ ] **Leaving the folder:** closing Cairn and moving the whole folder to another place, then starting it, shows the same data (it is portable).
- [ ] **Antivirus:** no warning from Defender after the first start. Note any message: ______
- [ ] The `NOTICE` file in the folder opens (Notepad) and starts with the Gemma notice.

## Result

Date: ______  Windows version: ______  Cairn version: ______

Problems found: ______
