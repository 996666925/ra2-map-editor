import { Client, onStart } from '@ra2-map-editor/mission-sdk/client';
import { MessageKind } from '../../shared/protocol';

// 界面入口仅影响本机显示，与 rules 共享消息类型。
onStart((): void => {
  Client.log('ui ready: ' + MessageKind.Ready.toString());
});
