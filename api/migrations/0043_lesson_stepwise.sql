-- 逐步設計: a lesson started from the home page's 逐步設計 card is listed in 我的作品 even
-- before anything is saved in its SOP. Lessons made any other way are listed once they have one.
ALTER TABLE lessons ADD COLUMN stepwise BOOLEAN NOT NULL DEFAULT false;
