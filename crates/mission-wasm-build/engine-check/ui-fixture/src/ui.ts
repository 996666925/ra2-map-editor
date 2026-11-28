import {
  Client,
  ClientEvent,
  SelectionChangedEvent,
  onClientEvent,
  onRender,
  onStart,
} from '@ra2-map-editor/mission-sdk/client';

// 验收工具派发一次 start、600 次 render，以及两次 SelectionChanged。
// 只累计计数，第 600 次渲染时输出一次；host 只需实现 client_log/client_frame。
let startCalls: i32 = 0;
let renderCalls: i32 = 0;
let selectionEvents: i32 = 0;

onStart((): void => {
  startCalls += 1;
});

onClientEvent<SelectionChangedEvent>(ClientEvent.SelectionChanged, (_event: SelectionChangedEvent): void => {
  selectionEvents += 1;
});

onRender((_dt: f32, _alpha: f32): void => {
  renderCalls += 1;
  if (renderCalls == 600) {
    // probe 帧号从 0 开始，因此第 600 次渲染对应 frame=599。
    Client.log('ui lifecycle start=' + startCalls.toString()
      + ' render=' + renderCalls.toString()
      + ' events=' + selectionEvents.toString()
      + ' frame=' + Client.frame().toString());
  }
});
