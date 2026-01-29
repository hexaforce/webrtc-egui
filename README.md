# WebRTC低遅延受信 GUI アプリケーション

eGuiを使用したWebRTC受信アプリケーションです。GStreamerのwebrtcsrcエレメントを使用して、低遅延でビデオとオーディオを受信します。

## 機能

- 🎥 リアルタイムビデオ表示
- 🔊 オーディオ再生
- 📊 接続状態とログの表示
- ⚡ 低遅延設定（20ms）
- 🎛️ GUIで簡単に開始/停止

## 必要な環境

### GStreamer

このアプリケーションはGStreamerを使用しています。以下のプラグインが必要です：

**macOS:**
```bash
brew install gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly
```

**Ubuntu/Debian:**
```bash
sudo apt-get install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    libgstreamer-plugins-bad1.0-dev gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly gstreamer1.0-libav
```

### Rust

Rustツールチェーン（1.70以上）が必要です：
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## 使い方

### 1. シグナリングサーバーを起動

gst-plugins-rsのシグナリングサーバーを起動します：

```bash
cd /path/to/gst-plugins-rs/net/webrtc/signalling
cargo run --bin gst-webrtc-signalling-server
```

### 2. 送信側を起動

gst-launchを使用してビデオとオーディオを送信します：

```bash
gst-launch-1.0 webrtcsink name=ws meta="meta,name=test" \
    videotestsrc ! ws. \
    audiotestsrc ! ws.
```

または、カメラとマイクから送信する場合：

**macOS:**
```bash
gst-launch-1.0 webrtcsink name=ws meta="meta,name=test" \
    avfvideosrc ! videoconvert ! ws. \
    osxaudiosrc ! audioconvert ! ws.
```

**Linux:**
```bash
gst-launch-1.0 webrtcsink name=ws meta="meta,name=test" \
    v4l2src ! videoconvert ! ws. \
    pulsesrc ! audioconvert ! ws.
```

### 3. 受信側GUIアプリを起動

このプロジェクトディレクトリで：

```bash
cargo run --release
```

GUIが起動したら、「▶️ 開始」ボタンをクリックしてストリームの受信を開始します。

## 設定

### シグナリングサーバーのURL

デフォルトではローカルホスト（`ws://127.0.0.1:8443`）に接続します。
異なるURLに接続する場合は、環境変数を設定してください：

```bash
GST_WEBRTC_SIGNALLING_SERVER_URI=ws://example.com:8443 cargo run
```

### ビデオコーデック

デフォルトでは H264 > VP8 の優先順位でコーデックを選択します。
src/main.rsの以下の行を変更することで優先順位を変更できます：

```rust
.property_from_str("video-codecs", "<H264, VP8>")
```

## トラブルシューティング

### macOSでビデオが表示されない

macOSでは、NSApplicationの初期化が必要です。コードには既に含まれていますが、
それでも問題がある場合は、環境変数を設定してみてください：

```bash
GST_DEBUG=3 cargo run
```

### オーディオが聞こえない

オーディオデバイスの設定を確認してください：

**macOS:**
```bash
system_profiler SPAudioDataType
```

**Linux:**
```bash
pactl list sinks
```

### 接続できない

1. シグナリングサーバーが起動しているか確認
2. 送信側が起動しているか確認
3. ファイアウォールの設定を確認
4. ログウィンドウでエラーメッセージを確認

## 開発

### ビルド

```bash
cargo build
```

### デバッグビルドで実行

```bash
cargo run
```

### リリースビルドで実行（最適化）

```bash
cargo run --release
```

## ライセンス

このプロジェクトは元の gst-plugins-rs のライセンスに従います。

## 参考

- [gst-plugins-rs](https://gitlab.freedesktop.org/gstreamer/gst-plugins-rs)
- [GStreamer](https://gstreamer.freedesktop.org/)
- [eGUI](https://github.com/emilk/egui)
