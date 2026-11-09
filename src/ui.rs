pub const INDEX_HTML: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Guest Meeting Demo</title>
    <style>
        :root {
            color-scheme: dark;
            --bg: #0f172a;
            --panel: #111827;
            --panel-2: #1f2937;
            --accent: #38bdf8;
            --text: #e5e7eb;
            --muted: #94a3b8;
            --danger: #f87171;
        }
        body {
            margin: 0;
            background: var(--bg);
            color: var(--text);
            font-family: Arial, sans-serif;
        }
        .container {
            max-width: 700px;
            margin: 3rem auto;
            padding: 2rem;
            background: var(--panel);
            border: 1px solid #334155;
            border-radius: 16px;
        }
        h1 { margin-top: 0; }
        .form-grid {
            display: grid;
            gap: 1rem;
            margin-top: 1.25rem;
        }
        label {
            display: grid;
            gap: 0.35rem;
            font-weight: 600;
        }
        input, button {
            font: inherit;
        }
        input {
            padding: 0.8rem 0.9rem;
            border-radius: 10px;
            border: 1px solid #475569;
            background: var(--panel-2);
            color: var(--text);
        }
        .row {
            display: flex;
            gap: 0.75rem;
            flex-wrap: wrap;
        }
        button {
            border: none;
            border-radius: 10px;
            background: var(--accent);
            color: #082f49;
            padding: 0.8rem 1.1rem;
            font-weight: 700;
            cursor: pointer;
        }
        button.secondary {
            background: #334155;
            color: var(--text);
        }
        .status {
            margin-top: 1rem;
            color: var(--muted);
            min-height: 1.5rem;
        }
        .invite {
            margin-top: 1rem;
            background: #020617;
            border: 1px solid #334155;
            border-radius: 10px;
            padding: 0.85rem 1rem;
            word-break: break-all;
            color: var(--text);
        }
        .hidden { display: none; }
    </style>
</head>
<body>
    <div class="container">
        <h1>Guest Meeting</h1>
        <p>Create a new meeting or join an existing one with a room code or invite link.</p>

        <div class="form-grid">
            <label>
                Display name
                <input id="displayName" type="text" maxlength="64" placeholder="Alex" />
            </label>

            <div class="row">
                <button id="createMeeting" type="button">Create meeting</button>
                <button id="joinMeeting" type="button" class="secondary">Join meeting</button>
            </div>

            <label>
                Room code or invite URL
                <input id="roomInput" type="text" placeholder="room code or https://.../?room=abc123" />
            </label>
        </div>

        <div id="status" class="status" role="status" aria-live="polite"></div>
        <div id="inviteBox" class="invite hidden"></div>
    </div>

    <script>
        const displayNameInput = document.getElementById('displayName');
        const roomInput = document.getElementById('roomInput');
        const statusEl = document.getElementById('status');
        const inviteBox = document.getElementById('inviteBox');

        function setStatus(message, isError = false) {
            statusEl.textContent = message;
            statusEl.style.color = isError ? '#f87171' : '#94a3b8';
        }

        function parseRoomFromInput(value) {
            if (!value) return { room: '', inviteUrl: '' };
            if (/^https?:\/\//i.test(value)) {
                try {
                    const url = new URL(value);
                    const room = url.searchParams.get('room') || url.pathname.split('/').pop();
                    return { room: room || '', inviteUrl: value };
                } catch (error) {
                    return { room: '', inviteUrl: '' };
                }
            }
            return { room: value.trim(), inviteUrl: '' };
        }

        async function createMeeting() {
            const displayName = displayNameInput.value.trim();
            if (!displayName) {
                setStatus('Please enter a display name first.', true);
                return;
            }

            setStatus('Creating meeting...');
            const response = await fetch('/api/meeting', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ display_name: displayName }),
            });
            const data = await response.json();
            if (!response.ok) {
                setStatus(data.error || 'Could not create meeting.', true);
                return;
            }

            inviteBox.textContent = data.invite_url;
            inviteBox.classList.remove('hidden');
            roomInput.value = data.room_id;
            setStatus(`Meeting created. Share this invite: ${data.room_id}`);
            navigator.clipboard?.writeText(data.invite_url).catch(() => {});
        }

        async function joinMeeting() {
            const displayName = displayNameInput.value.trim();
            const { room, inviteUrl } = parseRoomFromInput(roomInput.value);
            if (!displayName) {
                setStatus('Please enter your display name before joining.', true);
                return;
            }
            if (!room) {
                setStatus('Enter a room code or valid invite URL.', true);
                return;
            }

            setStatus('Requesting guest ticket...');
            const response = await fetch(`/api/meeting/${encodeURIComponent(room)}/ticket`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ display_name: displayName }),
            });
            const data = await response.json();
            if (!response.ok) {
                setStatus(data.error || 'Unable to join that meeting.', true);
                return;
            }

            const joinUrl = inviteUrl || data.invite_url;
            inviteBox.textContent = joinUrl;
            inviteBox.classList.remove('hidden');
            const ticket = new URL(joinUrl || `http://127.0.0.1:3000/?room=${room}`).searchParams.get('ticket') || data.ticket;
            const wsUrl = `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws?room=${encodeURIComponent(room)}&ticket=${encodeURIComponent(ticket)}`;
            setStatus(`Joining room ${room}...`);
            const socket = new WebSocket(wsUrl);
            socket.addEventListener('open', () => {
                setStatus(`Connected to room ${room}.`);
                window.location.href = `/?room=${encodeURIComponent(room)}&ticket=${encodeURIComponent(ticket)}`;
            });
            socket.addEventListener('error', () => {
                setStatus('Unable to connect to the meeting signaling socket.', true);
            });
        }

        document.getElementById('createMeeting').addEventListener('click', createMeeting);
        document.getElementById('joinMeeting').addEventListener('click', joinMeeting);

        const params = new URLSearchParams(window.location.search);
        const roomFromUrl = params.get('room');
        const ticketFromUrl = params.get('ticket');
        if (roomFromUrl) {
            roomInput.value = roomFromUrl;
        }
        if (ticketFromUrl) {
            inviteBox.textContent = `${window.location.origin}/?room=${roomFromUrl}&ticket=${ticketFromUrl}`;
            inviteBox.classList.remove('hidden');
        }
    </script>
</body>
</html>
"###;
