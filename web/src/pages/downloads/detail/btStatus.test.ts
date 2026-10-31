import { describe, expect, test } from 'bun:test'
import type { BtTrackerDto } from '../../../lib/rpc'
import { mapTrackerRows, nextTrackerSort, trackerCounts, trackerView } from './btStatus'
import type { TrackerFilter, TrackerSort } from './btStatus'

interface Fixture {
  trackers: BtTrackerDto[]
  counts: Record<TrackerFilter, number>
  cases: { name: string; filter: TrackerFilter; sort: TrackerSort; expect: number[] }[]
}

declare const Bun: { file(path: URL): { json(): Promise<unknown> } }
const fixture = (await Bun.file(new URL('../../../../../crates/downloads/tests/fixtures/bt_tracker_view_cases.json', import.meta.url)).json()) as Fixture
const rows = mapTrackerRows(fixture.trackers)

describe('tracker view (shared fixtures)', () => {
  test('counts', () => {
    expect(trackerCounts(rows)).toEqual(fixture.counts)
  })
  for (const item of fixture.cases) {
    test(item.name, () => {
      expect(trackerView(rows, item.filter, item.sort)).toEqual(item.expect)
    })
  }
})

describe('nextTrackerSort', () => {
  test('三态：升序 → 降序 → 无', () => {
    const asc = nextTrackerSort(null, 'peers')
    expect(asc).toEqual({ key: 'peers', desc: false })
    const desc = nextTrackerSort(asc, 'peers')
    expect(desc).toEqual({ key: 'peers', desc: true })
    expect(nextTrackerSort(desc, 'peers')).toBeNull()
  })
  test('换列从升序开始', () => {
    expect(nextTrackerSort({ key: 'peers', desc: true }, 'url')).toEqual({ key: 'url', desc: false })
  })
})
