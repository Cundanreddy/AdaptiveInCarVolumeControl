from flask import Flask, jsonify, request, render_template_string

app = Flask(__name__)

# In-memory state controlled by the UI
STATE = {
        "cabin_db": 60.0,
        "speed_kmh": 60.0,
}

HTML = """
<!doctype html>
<html>
    <head>
        <meta charset="utf-8" />
        <title>Speed / Cabin Noise UI</title>
        <style>
            body { font-family: Arial, sans-serif; margin: 2rem; }
            .row { margin-bottom: 1rem; }
            label { display:block; margin-bottom: .25rem; }
        </style>
    </head>
    <body>
        <h2>Control Panel</h2>
        <div class="row">
            <label for="cabin">Cabin noise (dB): <span id="cabin_val">60.0</span></label>
            <input id="cabin" type="range" min="30" max="100" step="0.1" value="60">
        </div>
        <div class="row">
            <label for="speed">Speed (km/h): <span id="speed_val">60.0</span></label>
            <input id="speed" type="range" min="0" max="200" step="0.1" value="60">
        </div>
        <button id="send">Send to server</button>

        <script>
            const cabin = document.getElementById('cabin');
            const speed = document.getElementById('speed');
            const cabinVal = document.getElementById('cabin_val');
            const speedVal = document.getElementById('speed_val');
            const send = document.getElementById('send');

            function updateLabels(){
                cabinVal.textContent = parseFloat(cabin.value).toFixed(1);
                speedVal.textContent = parseFloat(speed.value).toFixed(1);
            }
            cabin.addEventListener('input', updateLabels);
            speed.addEventListener('input', updateLabels);

            // Send update helper (used by interaction and the Send button)
            async function sendUpdate(body){
                try{
                    await fetch('/update', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify(body) });
                }catch(e){ console.warn('update failed', e); }
            }

            // When user moves a slider we immediately update the UI locally and
            // send the updated state to the server (debounced). While the user is
            // interacting the background poll will not overwrite the UI.
            let isUserInteracting = false;
            let sendTimeout = null;
            function scheduleSend(){
                if(sendTimeout) clearTimeout(sendTimeout);
                sendTimeout = setTimeout(() => {
                    const body = { cabin_db: parseFloat(cabin.value), speed_kmh: parseFloat(speed.value) };
                    sendUpdate(body);
                    isUserInteracting = false;
                }, 200); // 200ms debounce
            }

            cabin.addEventListener('input', () => { updateLabels(); isUserInteracting = true; scheduleSend(); });
            speed.addEventListener('input', () => { updateLabels(); isUserInteracting = true; scheduleSend(); });

            // Also support explicit Send button if desired (instant)
            send.addEventListener('click', async () => {
                const body = { cabin_db: parseFloat(cabin.value), speed_kmh: parseFloat(speed.value) };
                isUserInteracting = true;
                if(sendTimeout) clearTimeout(sendTimeout);
                await sendUpdate(body);
                isUserInteracting = false;
            });

            // poll current state every 500ms to initialize and keep UI in sync.
            // If the user is currently interacting we skip the update to avoid
            // reverting the slider while dragging.
            async function poll(){
                try{
                    const r = await fetch('/state');
                    const j = await r.json();
                    if(!isUserInteracting){
                        cabin.value = j.cabin_db; speed.value = j.speed_kmh; updateLabels();
                    }
                }catch(e){ console.warn('poll failed', e); }
                setTimeout(poll, 500);
            }
            poll();
        </script>
    </body>
</html>
"""


@app.route('/')
def index():
        return render_template_string(HTML)


@app.route('/state')
def state():
        return jsonify(STATE)


@app.route('/update', methods=['POST'])
def update():
        payload = request.get_json(force=True)
        if not payload:
                return jsonify({"error": "missing json"}), 400
        if 'cabin_db' in payload:
                try:
                        STATE['cabin_db'] = float(payload['cabin_db'])
                except Exception:
                        pass
        if 'speed_kmh' in payload:
                try:
                        STATE['speed_kmh'] = float(payload['speed_kmh'])
                except Exception:
                        pass
        return jsonify(STATE)


if __name__ == '__main__':
        # Bind to 127.0.0.1:5005 by default
        app.run(host='127.0.0.1', port=5005, debug=False)
