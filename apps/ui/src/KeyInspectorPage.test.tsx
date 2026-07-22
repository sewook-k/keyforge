import { fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { KeyInspectorPage } from './pages';

describe('KeyInspectorPage', () => {
  it('shows normalized key details and keeps only in-memory events', async () => {
    const user = userEvent.setup();
    render(<KeyInspectorPage />);

    const target = screen.getByRole('application', { name: '키 입력 검사 영역' });
    expect(target).toHaveFocus();

    fireEvent.keyDown(target, {
      key: 'Control',
      code: 'ControlLeft',
      keyCode: 17,
      location: 1,
      ctrlKey: true,
    });

    const details = screen.getByRole('region', { name: '최근 키 상세 정보' });
    expect(within(details).getAllByText('ControlLeft')).toHaveLength(3);
    expect(within(details).getByText('Control')).toBeInTheDocument();
    expect(within(details).getByText('17 / 0x11')).toBeInTheDocument();
    expect(within(details).getByText('왼쪽 (1)')).toBeInTheDocument();
    expect(within(details).getByText('Ctrl')).toBeInTheDocument();
    expect(screen.getByText('현재 눌린 키')).toBeInTheDocument();

    fireEvent.keyUp(target, {
      key: 'Control',
      code: 'ControlLeft',
      keyCode: 17,
      location: 1,
    });
    expect(screen.getByText('여기를 클릭하고 확인할 키를 누르세요')).toBeInTheDocument();
    expect(within(screen.getByRole('region', { name: '최근 키 상세 정보' })).getByText('key-up')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: '기록 지우기' }));
    expect(screen.getByText('아직 감지된 키가 없습니다')).toBeInTheDocument();
    expect(target).toHaveFocus();
  });
});
