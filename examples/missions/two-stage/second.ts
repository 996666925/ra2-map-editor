import { defineMission, event, action } from './sdk';
import { map, House } from './map.generated';

export default defineMission(map, { id: 'second_stage', player: House.Americans }, m => {
  const ready = m.globalFlag(49); // 只引用，保留上关保存的状态
  const message = m.defineText('carry', '上一关的增援任务已完成。');
  m.rule('inherited', {
    when: event.all(event.globalSet(ready), event.sinceEnabled(1)), once: true,
    actions: [action.message(message)],
  });
  m.rule('finish', {
    when: event.sinceScenarioStart(60), once: true,
    actions: [action.winPlayer()],
  });
});
