import asyncio
import json
import os
import sys
import websockets

async def get_exposed_entities():
    token = os.getenv("HOMEASSISTANT_TOKEN")
    if not token:
        print("Error: HOMEASSISTANT_TOKEN is not set.", file=sys.stderr)
        return

    # User environment uses 192.168.1.30 as the HA host
    url = "ws://192.168.1.30:8123/api/websocket"
    
    try:
        async with websockets.connect(url) as websocket:
            # 1. 接続直後に auth_required を受信
            auth_required = json.loads(await websocket.recv())
            if auth_required.get("type") != "auth_required":
                print(f"Error: Expected auth_required, got {auth_required.get('type')}", file=sys.stderr)
                return

            # 2. 認証メッセージを送信
            await websocket.send(json.dumps({
                "type": "auth",
                "access_token": token
            }))

            # 3. auth_ok を受信
            auth_result = json.loads(await websocket.recv())
            if auth_result.get("type") != "auth_ok":
                print(f"Error: Auth failed: {auth_result}", file=sys.stderr)
                return

            # 4. exposed アイテムの一覧を取得 (id=1)
            await websocket.send(json.dumps({
                "id": 1,
                "type": "homeassistant/expose_entity/list"
            }))

            # 5. 結果を受信して表示
            result_msg = json.loads(await websocket.recv())
            if result_msg.get("success"):
                exposed_entities = result_msg.get("result", {}).get("exposed_entities", {})
                print(json.dumps(exposed_entities, indent=2, ensure_ascii=False))
            else:
                print(f"Error: Command failed: {result_msg}", file=sys.stderr)

    except Exception as e:
        print(f"Error connecting to WebSocket: {e}", file=sys.stderr)

if __name__ == "__main__":
    asyncio.run(get_exposed_entities())
