# `UPowerDeviceType`

**Module:** `HTMShell.Services.UPower` | **Kind:** Finite state

`item.type` and `battery.type` provide human-readable text and one finite token.

## Tokens

```text
unknown
line-power
battery
ups
monitor
mouse
keyboard
pda
phone
media-player
tablet
computer
gaming-input
pen
touchpad
modem
network
headset
speakers
headphones
video
other-audio
remote-control
printer
scanner
camera
wearable
toy
bluetooth-generic
```

Text values are `Unknown`, `Line power`, `Battery`, `UPS`, `Monitor`, `Mouse`, `Keyboard`, `PDA`, `Phone`, `Media player`, `Tablet`, `Computer`, `Gaming input`, `Pen`, `Touchpad`, `Modem`, `Network`, `Headset`, `Speakers`, `Headphones`, `Video`, `Other audio`, `Remote control`, `Printer`, `Scanner`, `Camera`, `Wearable`, `Toy`, and `Bluetooth`.

Unknown future UPower type numbers map to `unknown`.

## Usage

```css
.device[data-htm-state="bluetooth-generic"] {
  opacity: 0.8;
}
```

## See also

- [`UPowerDevice`](UPowerDevice.md)
