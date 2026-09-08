pub const INDEX_HTML: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>WebRTC Signaling Demo</title>
    <style>
        body {
            font-family: Arial, sans-serif;
            margin: 2rem;
            background: #0f172a;
            color: #e2e8f0;
        }
        .container {
            max-width: 960px;
            margin: 0 auto;
        }
        .grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
            gap: 1rem;
        }
        .video-panel {
            position: relative;
        }
        video {
            width: 100%;
            max-height: 320px;
            background: #020617;
            border: 1px solid #334155;
            border-radius: 12px;
        }
        button {
            background: #38bdf8;
            border: none;
            border-radius: 8px;
            color: #082f49;
            padding: 0.75rem 1rem;
            font-weight: 700;
            cursor: pointer;
            margin-right: 0.5rem;
        }
        button.media-control {
            min-width: 8rem;
        }
        button.icon-control {
            position: absolute;
            top: 0.75rem;
            right: 0.75rem;
            z-index: 1;
            min-width: 0;
            width: 2.75rem;
            height: 2.75rem;
            padding: 0;
            margin: 0;
            border: 1px solid rgba(255, 255, 255, 0.35);
            border-radius: 50%;
            background: rgba(15, 23, 42, 0.82);
            color: #f8fafc;
        }
        button.icon-control .control-icon {
            margin: 0;
        }
        button.icon-control .control-label {
            position: absolute;
            width: 1px;
            height: 1px;
            padding: 0;
            margin: -1px;
            overflow: hidden;
            clip: rect(0, 0, 0, 0);
            white-space: nowrap;
            border: 0;
        }
        button.icon-control.off {
            background: rgba(185, 28, 28, 0.9);
        }
        button.media-control.off {
            background: #475569;
            color: #e2e8f0;
        }
        .control-icon {
            display: inline-block;
            width: 1.25rem;
            margin-right: 0.35rem;
            font-size: 1.1rem;
            vertical-align: -0.1rem;
        }
        textarea {
            width: 100%;
            min-height: 120px;
            border-radius: 10px;
            border: 1px solid #334155;
            background: #020617;
            color: #e2e8f0;
            padding: 0.75rem;
            box-sizing: border-box;
            margin-top: 0.75rem;
        }
        #status {
            margin-top: 1rem;
            color: #7dd3fc;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>WebRTC Signaling Demo</h1>
        <p>Open this page in two tabs to test call negotiation through the Axum signaling server.</p>

        <div class="grid">
            <div class="video-panel">
                <video id="localVideo" autoplay muted playsinline></video>
                <button id="toggleMic" class="media-control icon-control" type="button" title="Toggle microphone" aria-label="Toggle microphone" aria-pressed="true">
                    <span class="control-icon" aria-hidden="true">🎙</span><span class="control-label">Mic on</span>
                </button>
            </div>
            <div>
                <video id="remoteVideo" autoplay playsinline></video>
            </div>
        </div>

        <div style="margin-top: 1rem;">
            <button id="startButton">Start Call</button>
            <button id="toggleCam" class="media-control" type="button" title="Toggle camera" aria-pressed="true">
                <span class="control-icon" aria-hidden="true">📹</span><span class="control-label">Camera on</span>
            </button>
            <button id="toggleScreen" class="media-control" type="button" title="Share screen" aria-pressed="false">
                <span class="control-icon" aria-hidden="true">🖥</span><span class="control-label">Share screen</span>
            </button>
        </div>

        <div id="status">Connecting to signaling server...</div>
        <textarea id="log" readonly></textarea>
    </div>

    <script>
        const localVideo = document.getElementById('localVideo');
        const remoteVideo = document.getElementById('remoteVideo');
        const logArea = document.getElementById('log');
        const status = document.getElementById('status');
        const startButton = document.getElementById('startButton');

        const room = 'demo-room';
        let peerId = null;
        const ws = new WebSocket(`ws://${location.host}/ws?room=${room}`);
        let peerConnection;
        let localStream;
        let cameraTrack;
        let screenStream;

        function appendLog(message) {
            const time = new Date().toLocaleTimeString();
            logArea.value += `[${time}] ${message}\n`;
            logArea.scrollTop = logArea.scrollHeight;
        }

        function updateMediaControl(buttonId, track, label, onIcon, offIcon) {
            const button = document.getElementById(buttonId);
            const enabled = Boolean(track && track.enabled);
            button.classList.toggle('off', !enabled);
            button.setAttribute('aria-pressed', String(enabled));
            button.title = `${enabled ? 'Turn off' : 'Turn on'} ${label.toLowerCase()}`;
            button.querySelector('.control-icon').textContent = enabled ? onIcon : offIcon;
            button.querySelector('.control-label').textContent = `${label} ${enabled ? 'on' : 'off'}`;
        }

        function updateMediaControls() {
            updateMediaControl('toggleMic', localStream?.getAudioTracks()[0], 'Mic', '🎙', '🔇');
            updateMediaControl('toggleCam', localStream?.getVideoTracks()[0], 'Camera', '📹', '🚫');
            const screenButton = document.getElementById('toggleScreen');
            const sharing = Boolean(screenStream);
            screenButton.classList.toggle('off', sharing);
            screenButton.setAttribute('aria-pressed', String(sharing));
            screenButton.title = sharing ? 'Stop sharing screen' : 'Share screen';
            screenButton.querySelector('.control-icon').textContent = sharing ? '⏹' : '🖥';
            screenButton.querySelector('.control-label').textContent = sharing ? 'Stop sharing' : 'Share screen';
        }

        async function initMedia() {
            localStream = await navigator.mediaDevices.getUserMedia({
                video: true,
                audio: true,
            });
            cameraTrack = localStream.getVideoTracks()[0];
            localVideo.srcObject = localStream;
            updateMediaControls();
        }

        function configurePeerConnection() {
            peerConnection = new RTCPeerConnection({
                iceServers: [{ urls: 'stun:stun.l.google.com:19302' }],
            });

            localStream.getTracks().forEach(track => {
                peerConnection.addTrack(track, localStream);
            });

            peerConnection.ontrack = (event) => {
                remoteVideo.srcObject = event.streams[0];
                appendLog('Remote video stream received.');
            };

            peerConnection.onicecandidate = (event) => {
                if (!event.candidate) return;
                ws.send(JSON.stringify({
                    type: 'candidate',
                    room,
                    from: peerId,
                    to: '',
                    candidate: {
                        candidate: event.candidate.candidate,
                        sdpMid: event.candidate.sdpMid,
                        sdpMLineIndex: event.candidate.sdpMLineIndex,
                    },
                }));
                appendLog('ICE candidate sent.');
            };

            peerConnection.onconnectionstatechange = () => {
                status.textContent = `Connection state: ${peerConnection.connectionState}`;
                appendLog(`Connection state changed to ${peerConnection.connectionState}.`);
            };
        }

        async function createOffer() {
            if (!peerConnection) {
                configurePeerConnection();
            }
            const offer = await peerConnection.createOffer();
            await peerConnection.setLocalDescription(offer);
            ws.send(JSON.stringify({
                type: 'offer',
                room,
                from: peerId,
                to: '',
                sdp: offer.sdp,
            }));
            appendLog('Offer sent.');
        }

        async function replaceVideoTrack(track) {
            const sender = peerConnection?.getSenders().find((item) => item.track?.kind === 'video');
            if (!sender) return;
            await sender.replaceTrack(track);
            localStream.removeTrack(localStream.getVideoTracks()[0]);
            localStream.addTrack(track);
            localVideo.srcObject = localStream;
            await createOffer();
        }

        async function stopScreenSharing() {
            if (!screenStream) return;
            const previousScreenStream = screenStream;
            screenStream = null;
            previousScreenStream.getTracks().forEach((track) => track.stop());
            await replaceVideoTrack(cameraTrack);
            updateMediaControls();
            appendLog('Screen sharing stopped.');
        }

        async function startScreenSharing() {
            if (!peerConnection) {
                configurePeerConnection();
            }
            screenStream = await navigator.mediaDevices.getDisplayMedia({ video: true });
            const screenTrack = screenStream.getVideoTracks()[0];
            screenTrack.addEventListener('ended', () => {
                stopScreenSharing().catch((error) => appendLog(error.message));
            }, { once: true });
            await replaceVideoTrack(screenTrack);
            updateMediaControls();
            appendLog('Screen sharing started.');
        }

        async function handleSignal(message) {
            const data = JSON.parse(message.data);
            if (!data || !data.type) return;

            if (data.to && data.to !== peerId) {
                return;
            }

            appendLog(`Received signal: ${data.type}`);

            if (data.type === 'peer-joined') {
                status.textContent = 'Peer connected through the media server.';
                return;
            }

            if (data.type === 'welcome') {
                peerId = data.peer_id;
                status.textContent = `Connected as ${peerId}.`;
                appendLog(`Assigned peer ID ${peerId}.`);
                if (localStream) {
                    createOffer().catch((err) => appendLog(err.message));
                }
                return;
            }

            if (data.type === 'offer') {
                if (!peerConnection) {
                    configurePeerConnection();
                }
                await peerConnection.setRemoteDescription({ type: 'offer', sdp: data.sdp });
                const answer = await peerConnection.createAnswer();
                await peerConnection.setLocalDescription(answer);
                ws.send(JSON.stringify({
                    type: 'answer',
                    room,
                    from: peerId,
                    to: data.from,
                    sdp: answer.sdp,
                }));
                appendLog('Answer sent.');
                return;
            }

            if (data.type === 'answer') {
                await peerConnection.setRemoteDescription({ type: 'answer', sdp: data.sdp });
                appendLog('Remote answer applied.');
                return;
            }

            if (data.type === 'candidate' && data.candidate) {
                if (!peerConnection) {
                    configurePeerConnection();
                }
                await peerConnection.addIceCandidate({
                    candidate: data.candidate.candidate,
                    sdpMid: data.candidate.sdpMid,
                    sdpMLineIndex: data.candidate.sdpMLineIndex,
                });
                appendLog('ICE candidate added.');
            }
        }

        ws.onopen = async () => {
            status.textContent = 'Connected to signaling server.';
            try {
                await initMedia();
            } catch (error) {
                status.textContent = 'Microphone/camera access denied. Grant permissions and refresh.';
                appendLog(error.message);
            }
        };

        ws.onmessage = handleSignal;

        ws.onclose = () => {
            status.textContent = 'WebSocket disconnected.';
            appendLog('Connection closed. Refresh the page to reconnect.');
        };

        startButton.addEventListener('click', async () => {
            if (!peerId) {
                appendLog('Waiting for the server to assign a peer ID.');
                return;
            }
            if (!localStream) {
                await initMedia();
            }
            if (!peerConnection) {
                configurePeerConnection();
            }
            await createOffer();
        });

        document.getElementById('toggleMic').addEventListener('click', () => {
            const track = localStream?.getAudioTracks()[0];
            if (track) {
                track.enabled = !track.enabled;
                updateMediaControls();
            }
        });

        document.getElementById('toggleCam').addEventListener('click', () => {
            const track = localStream?.getVideoTracks()[0];
            if (track) {
                track.enabled = !track.enabled;
                updateMediaControls();
            }
        });

        document.getElementById('toggleScreen').addEventListener('click', async () => {
            try {
                if (screenStream) {
                    await stopScreenSharing();
                } else {
                    await startScreenSharing();
                }
            } catch (error) {
                appendLog(error.message);
                updateMediaControls();
            }
        });
    </script>
</body>
</html>
"#;
