import {
  BuildingType,
  clearInterval,
  clearTimeout,
  console,
  setInterval,
  setTimeout,
  TimerId,
  Unit,
  UnitType,
} from '@ra2-map-editor/mission-sdk';
import {
  Cell,
  Every,
  EventRecord,
  House,
  Mission,
  NO_HANDLE,
  Rules,
  Trigger,
  Var,
  frame,
  fail,
  log,
  onFrame,
  onRules,
  onScenarioStart,
} from '@vera20k/sdk';

let oneShotFired: i32 = 0;
let jsIntervalCalls: i32 = 0;
let jsIntervalId: TimerId = 0;

function onRepeatingTimer(_event: EventRecord): void {
  Var.local(7).set(1);
  if (oneShotFired == 1) Var.local(8).set(1);
  log('mission timer ' + frame().toString());
}

function onOneShotTimer(_event: EventRecord): void {
  oneShotFired = 1;
}

function onJsTimeout(): void {
  Var.local(10).set(1);
  console.log('console.log timer fired');
}

function onJsInterval(): void {
  jsIntervalCalls += 1;
  if (jsIntervalCalls == 3) {
    Var.local(11).set(1);
    clearInterval(jsIntervalId);
  }
}

function onCanceledJsTimeout(): void {
  Var.local(12).set(1);
}

onRules((): void => {
  Rules.set('E1', 'Strength', '237');
});

onScenarioStart((): void => {
  const rifleman = UnitType.resolve(UnitType.Rifleman);
  if (rifleman == NO_HANDLE) fail('unit type facade did not resolve E1');
  const spawned = Unit.spawn(rifleman, House.fromIndex(0), Cell.at(1, 1));
  Unit.setMission(spawned, Mission.Guard);
  if (BuildingType.AlliedPowerPlant != 'GAPOWR') fail('building type facade mismatch');
  Var.local(5).set(1);
  log('mission ready');
  console.info('console.info ready');
  console.debug('console.debug ready');
  console.warn('console.warn ready');
  console.error('console.error ready');
  console.assert(true, 'console.assert should stay quiet');
  setTimeout(onJsTimeout, 10);
  jsIntervalId = setInterval(onJsInterval, 7);
  const canceled = setTimeout(onCanceledJsTimeout, 5);
  clearTimeout(canceled);
  Trigger.every(15).enqueue<EventRecord>(onRepeatingTimer).done();
  Trigger.every(20).once().enqueue<EventRecord>(onOneShotTimer).done();
});

onFrame(new Every(1), (): void => {
  if (frame() == 600) Var.local(9).set(1);
});
