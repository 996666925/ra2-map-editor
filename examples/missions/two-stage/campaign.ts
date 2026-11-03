import { defineCampaign } from './sdk';

// 提供 maps/first.map、maps/second.map，在同目录刷新 sdk.ts/map.generated.ts。
export default defineCampaign({
  id: 'TWO_STAGE', entry: 'first.map',
  missions: [
    { file: 'first.map', source: 'maps/first.map', script: 'first.ts',
      title: '第一关：守住阵地', briefing: '等待援军抵达，准备反击。', next: 'second.map' },
    { file: 'second.map', source: 'maps/second.map', script: 'second.ts',
      title: '第二关：反击', briefing: '第一关完成的目标会保留。', endOfGame: true },
  ],
});
