/** 收藏與改編的次數：只有數字，沒有誰。 */
export function Counts({ saves, remixes }: { saves: number; remixes: number }) {
  return (
    <span className="com-counts">
      <span aria-label={`${saves} 次收藏`}>收藏 {saves}</span>
      <span aria-label={`${remixes} 次改編`}>改編 {remixes}</span>
    </span>
  );
}
