import { log, onScenarioStart } from '@ra2-map-editor/mission-sdk';
import { MessageKind } from '../../shared/protocol';

onScenarioStart((): void => {
  log('rules ready: ' + MessageKind.Ready.toString());
});
