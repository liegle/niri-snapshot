# Niri-snapshot

Simple logic printing current output, workspace and window state of [niri][1] every time receiving an event from [niri-ipc][2]'s event-stream.
Developed in order to make taskbar in [eww][3]. Maybe useful in other bar solutions.
Now it is almost usable but may encounter bugs.

[1]: https://github.com/niri-wm/niri
[2]: https://crates.io/crates/niri-ipc
[3]: https://github.com/elkowar/eww

## Usage
Simply run the binary: `niri-snapshot` to print json.

The json will be printed in one line, with format like:
```json
{
    "focused_workspace_id": 1,
    "focused_window_id": 1,
    "HDMI-A-1": [
        {
            "id": 1,
            "active_window_id": 1,
            "urgent": false,
            "active": true,
            "columns": [
                [
                    {
                        "id": 1,
                        "title": "firefox",
                        "urgent": false,
                        "icon": "/usr/share/icons/hicolor/16x16/apps/firefox.png"
                    }
                ]
            ],
            "floatings": []
        }
    ]
}
```

Currently there is also a command to switch niri workspace with id: `niri-snapshot ws <ID>`

It will be removed when this [issue][1] is complete: 

[1]: https://github.com/niri-wm/niri/issues/647

## Example Eww Config

```yuck
(deflisten NIRI_SNAPSHOT
    :initial '{"focused_workspace_id":-1,"focused_window_id":-1}'
    `niri-snapshot`)
(defwidget niri-w [w]
    (tooltip
        (box
            :class 'tooltip'
            { w.title })
        (eventbox
            :width 24
            :class { w.urgent ? 'niriwurgent' : w.id == NIRI_SNAPSHOT.focused_window_id ? 'niriwfocused' : 'niriw' }
            :onclick 'niri msg action focus-window --id ${w.id}'
            (literal
                :valign 'center'
                :content { w.icon == '' ? '"󰘔"' : '(image :image-width 16 :image-height 16 :icon-size 16 :path { "${w.icon}" })' }))))
(defwidget niri-col [col]
    (box
        :class 'niricol'
        :visible { arraylength(col) > 0 }
        (for w in col
            (niri-w
                :w { w }))))
(defwidget niri-ws [ws]
    (box
        :class { ws.active ? 'niriws' : '' }
        :space-evenly false
        (tooltip
            (chart
                :length 15
                :header ''
                :list '[{"key":"ID","values":[${ws.id}]},{"key":"COLUMNS","values":[${arraylength(ws.columns)}]}]')
            (eventbox
                :width 32
                :class { ws.urgent ? 'niriwsiconurgent' : ws.id == NIRI_SNAPSHOT.focused_workspace_id ? 'niriwsiconfocused' : 'niriwsicon' }
                ; https://github.com/niri-wm/niri/issues/647
                :onclick { ws.active ? '' : 'niri-snapshot ws ${ws.id}' }
                { ws.active ? '󰜋' : '' }))
        (box
            :spacing 4
            :visible { ws.active }
            :space-evenly false
            (for col in { ws.columns }
                (niri-col
                    :col { col }))
            (niri-col
                :col { ws.floatings }))))
(defwidget niri [output]
    (box
        :class 'niri'
        :space-evenly false
        (for ws in { NIRI_SNAPSHOT.outputs?.['${output}'] }
            (niri-ws
                :ws { ws }))))
```
