# Niri-snapshot

Simple sync logic printing current output, workspace and window state of [niri][1] every time receiving an event from [niri-ipc][2]'s event-stream.
Developed in order to make taskbar in [eww][3]. Maybe useful in other bar solutions.
Now it is almost usable but may encounter bugs.

[1]: https://github.com/niri-wm/niri
[2]: https://crates.io/crates/niri-ipc
[3]: https://github.com/elkowar/eww

## Example Usage

```yuck
(deflisten NIRI_SNAPSHOT
    :initial '{"focused_workspace_id":-1,"focused_window_id":-1,outputs:{}}'
    `path/to/niri-snapshot`)
(defwidget wm-column [column]
    (box
        :class 'wmcolumn'
        (for w in column
            (tooltip
                (box
                    :class 'tooltip'
                    { w.title })
                (eventbox
                    :width 24
                    :class { w.urgent ? 'wmwurgent' : w.id == NIRI_SNAPSHOT.focused_window_id ? 'wmwfocused' : 'wmw' }
                    :onclick 'niri msg action focus-window --id ${w.id}'
                    (literal
                        :valign 'center'
                        :content { w.icon == '' ? '"󰘔"' : '(image :image-width 16 :image-height 16 :icon-size 16 :path { "${w.icon}" })' }))))))
(defwidget wm-ws [ws]
    (box
        :class { ws.active ? 'wmws' : '' }
        :space-evenly false
        (tooltip
            (box
                :class 'tooltip'
                'Id: ${ws.id}')
            (eventbox
                :width 32
                :class { ws.urgent ? 'wmwsiconurgent' : ws.id == NIRI_SNAPSHOT.focused_workspace_id ? 'wmwsiconfocused' : 'wmwsicon' }
                ; https://github.com/niri-wm/niri/issues/647
                :onclick { ws.active ? '' : 'path/to/niri-snapshot ws ${ws.id}' }
                { ws.active ? '󰜋' : '' }))
        (box
            :spacing 4
            :space-evenly false
            :visible { ws.active && arraylength(ws.columns) > 0 }
            (for column in { ws.columns }
                (wm-column
                    :column { column })))))
(defwidget wm [output]
    (box
        :class 'wm'
        :space-evenly false
        (for ws in { NIRI_SNAPSHOT.outputs['${output}'] }
            (wm-ws
                :ws { ws }))))
```
