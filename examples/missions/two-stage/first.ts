import { defineMission, event, action, order } from './sdk';
import { map, House, Unit, Waypoint } from './map.generated';

// 两张地图均须有 Americans/Russians、0/1 航点，以及 E2 类型。
export default defineMission(map, { id: 'first_stage', player: House.Americans }, m => {
  const ready = m.globalFlag(49);
  const timer = m.defineText('timer', '下一波增援');
  const message = m.defineText('ready', '援军已集结，任务完成。');
  const entry = m.aliasWaypoint('landing', m.waypoint(Waypoint.WP0));
  const waves = m.ruleRef('waves');

  m.source('first.ts', () => {
    const team = m.team('reinforcements', {
      house: m.house(House.Americans), waypoint: entry,
      units: [{ type: m.unit(Unit.E2), count: 3 }],
      orders: [order.moveTo(m.waypoint(Waypoint.WP1)), order.hunt()],
    });
    m.rule('intro', {
      when: event.sinceEnabled(1), once: true,
      actions: [action.setCountdown(30), action.setCountdownTitle(timer)],
    });
    m.rule('waves', {
      when: event.sinceEnabled(30), once: false,
      actions: [action.reinforce(team, entry), action.setCountdown(30), action.setCountdownTitle(timer)],
    });
    m.rule('complete', {
      when: event.sinceScenarioStart(95), once: true,
      actions: [action.disableRule(waves), action.stopCountdown(), action.setGlobal(ready),
        action.message(message), action.winPlayer()],
    });
  });
});
